//! Read-only package explanation assembled from the Hangar fact stores.
//!
//! The CLI is deliberately a renderer. Dependency edges come from the
//! closure graph, liveness comes from lock/lifecycle roots, and rebuild facts
//! come from the stored action identity and build attempt record.

use super::{entry_action_key, ClosureGraph, Lifecycle, ProducerRecord, Roots, StoreEntry};
use crate::{BuildDebug, ProviderFacts, SemanticLock, Syntax, JSON};
use jet_foundation::Report::ReportEnvelope;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplainLens {
    All,
    WhyDepends,
    WhatDepends,
    Closure,
    WhyLive,
    Rebuild,
}

impl ExplainLens {
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "why-depends" | "dependency" => Self::WhyDepends,
            "what-depends" => Self::WhatDepends,
            "closure" => Self::Closure,
            "why-live" | "liveness" => Self::WhyLive,
            "rebuild" | "rebuild-reason" => Self::Rebuild,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::WhyDepends => "why-depends",
            Self::WhatDepends => "what-depends",
            Self::Closure => "closure",
            Self::WhyLive => "why-live",
            Self::Rebuild => "rebuild",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainReport {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub reference: String,
    pub output_hash: String,
    pub output_path: String,
    pub bin_path: String,
    pub rlib_path: String,
    pub receipt: String,
    pub realized_at: u64,
    pub last_used_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainObject {
    pub digest: String,
    pub path: String,
    pub external: bool,
    pub known: bool,
    pub owners: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExplainGraph {
    pub direct_dependencies: Vec<ExplainObject>,
    pub dependencies: Vec<ExplainObject>,
    pub direct_referrers: Vec<ExplainObject>,
    pub referrers: Vec<ExplainObject>,
    pub closure: Vec<ExplainObject>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainProvider {
    pub provider: String,
    pub immutable_source: String,
    pub source_digest: String,
    pub toolchain_facts: String,
    pub policy_facts: String,
    pub producer_facts: BTreeMap<String, String>,
    pub provider_facts: Option<ProviderFacts>,
    pub locked_provider_facts: Option<ProviderFacts>,
    pub profile_facts: Vec<ExplainProfile>,
    pub native_document: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainProfile {
    pub profile: String,
    pub generation: u64,
    pub output_hash: String,
    pub provider_facts: ProviderFacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainRoot {
    pub kind: String,
    pub id: String,
    pub producer: String,
    pub phase: String,
    pub label: String,
    pub reference: String,
    pub targets: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExplainLiveness {
    pub live: bool,
    pub roots: Vec<ExplainRoot>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainAttempt {
    pub id: String,
    pub package: String,
    pub reference: String,
    pub provider: String,
    pub status: String,
    pub failed_step: usize,
    pub scratch_dir: String,
    pub log_dir: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExplainRebuild {
    pub decision: String,
    pub reason: String,
    pub action_key: String,
    pub cache_identity: BTreeMap<String, String>,
    pub cache_admissions: Vec<ExplainCacheAdmission>,
    pub checks: BTreeMap<String, String>,
    pub attempt: Option<ExplainAttempt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainCacheAdmission {
    pub role: String,
    pub decision: String,
    pub builder: String,
    pub provenance: String,
    pub receipt_version: Option<u64>,
    pub receipt_expires_unix: Option<u64>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageExplain {
    pub schema: String,
    pub query: String,
    pub lens: String,
    pub entry: Option<ExplainEntry>,
    pub provider: Option<ExplainProvider>,
    pub graph: ExplainGraph,
    pub liveness: ExplainLiveness,
    pub rebuild: ExplainRebuild,
    pub reports: Vec<ExplainReport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyLocation {
    pub path: String,
    pub line: Option<usize>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyRequesting {
    pub env_file: WhyLocation,
    pub lock_file: WhyLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyOrigin {
    pub catalog: String,
    pub endpoint: String,
    pub cache_endpoint: String,
    pub signature_chain: String,
    pub source: String,
    pub source_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyTrust {
    pub grade: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyDisk {
    pub bytes: Option<u64>,
    pub objects: usize,
}

/// The compact, package-shaped projection used by `jetpack why`.
///
/// This intentionally keeps the command a renderer over the existing
/// producer, receipt, lock, and closure records. `PackageExplain` remains the
/// detailed diagnostic surface; this projection only selects the facts needed
/// to answer the common provenance question in one screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageWhy {
    pub query: String,
    pub package: String,
    pub available: bool,
    pub requesting: WhyRequesting,
    pub origin: WhyOrigin,
    pub trust: WhyTrust,
    pub disk: WhyDisk,
    pub dependents: Vec<String>,
    pub receipt: String,
}

/// Explain one package through the production Store, closure, lifecycle, and
/// build-debug records. `None` means neither a realized entry nor a recorded
/// build attempt exists for the query.
pub fn explain_package(
    roots: &Roots,
    query: &str,
    lens: ExplainLens,
) -> std::io::Result<Option<PackageExplain>> {
    // Explain is an observation surface. It must preserve the damaged state
    // long enough to report the missing proof instead of replaying recovery or
    // rewriting a receipt while answering a question.
    let entries = super::list_read_only(roots);
    let entry = find_entry(&entries, query)?;
    if entry.is_none() {
        let package = package_name(query);
        let attempt = match BuildDebug::latest(&roots.hangar_dir(), &package) {
            Ok(attempt) => attempt,
            Err(error) => {
                return Err(std::io::Error::other(format!(
                    "could not read build attempt for `{query}`: {error}"
                )))
            }
        };
        let Some(attempt) = attempt else {
            return Ok(None);
        };
        let mut reports = vec![ExplainReport {
            kind: "loss".to_string(),
            message: format!(
                "no realized StoreEntry exists for `{query}`; dependency, closure, and liveness facts are unavailable"
            ),
        }];
        let rebuild = rebuild_without_entry(&attempt);
        reports.push(ExplainReport {
            kind: "loss".to_string(),
            message: rebuild.reason.clone(),
        });
        return Ok(Some(PackageExplain {
            schema: jet_foundation::Report::REPORT_SCHEMA.to_string(),
            query: query.to_string(),
            lens: lens.label().to_string(),
            entry: None,
            provider: None,
            graph: ExplainGraph::default(),
            liveness: ExplainLiveness {
                reason: "no realized StoreEntry can be matched to a root".to_string(),
                ..Default::default()
            },
            rebuild,
            reports,
        }));
    }
    let entry = entry.expect("entry checked above");

    let mut reports = Vec::new();
    let graph = load_graph(roots, &mut reports);
    let receipt_ok = receipt_projection(roots, &entry, graph.as_ref(), &mut reports);
    let provider = provider_projection(&entry, &mut reports);
    let provider = provider.map(|mut provider| {
        provider.locked_provider_facts = locked_provider_facts(&entry, &mut reports);
        provider.profile_facts = profile_provider_facts(&entry, &mut reports);
        if let (Some(source), Some(locked)) = (
            provider.provider_facts.as_ref(),
            provider.locked_provider_facts.as_ref(),
        ) {
            compare_provider_identity(source, locked, &mut reports);
        }
        provider
    });
    let graph_view = graph_projection(&entries, &entry, graph.as_ref(), &mut reports);
    let liveness = liveness_projection(roots, &entry, graph.as_ref(), &mut reports);
    let rebuild = rebuild_projection(
        roots,
        &entry,
        graph.as_ref(),
        provider.as_ref(),
        receipt_ok,
        &mut reports,
    );

    Ok(Some(PackageExplain {
        schema: jet_foundation::Report::REPORT_SCHEMA.to_string(),
        query: query.to_string(),
        lens: lens.label().to_string(),
        entry: Some(entry_projection(&entry)),
        provider,
        graph: graph_view,
        liveness,
        rebuild,
        reports,
    }))
}

/// Explain the package facts that belong on the one-screen `why` surface.
/// This is read-only: the detailed explanation path already uses the
/// non-repairing Store projection and closure readers.
pub fn why_package(roots: &Roots, query: &str) -> std::io::Result<Option<PackageWhy>> {
    let Some(explanation) = explain_package(roots, query, ExplainLens::All)? else {
        return Ok(None);
    };
    PackageWhy::from_explain(roots, explanation).map(Some)
}

impl PackageWhy {
    fn from_explain(roots: &Roots, explanation: PackageExplain) -> std::io::Result<Self> {
        let package = explanation
            .entry
            .as_ref()
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| package_name(&explanation.query));
        let mut names = vec![package.clone()];
        let mut qualified = vec![explanation.query.clone()];
        if let Some(entry) = &explanation.entry {
            names.push(entry.name.clone());
            qualified.push(entry.reference.clone());
        }
        names.sort();
        names.dedup();
        qualified.sort();
        qualified.dedup();
        let requesting = requesting_projection(&names, &qualified)?;

        let (provider, facts, source, source_digest) = explanation
            .provider
            .as_ref()
            .map(|provider| {
                (
                    provider.provider.clone(),
                    Some(&provider.producer_facts),
                    provider.immutable_source.clone(),
                    provider.source_digest.clone(),
                )
            })
            .unwrap_or_else(|| (String::new(), None, String::new(), String::new()));
        let origin = origin_projection(roots, &provider, facts, &source, &source_digest);
        let trust = trust_projection(&provider, facts);
        let disk = disk_projection(&explanation.graph);
        let dependents = dependent_projection(&explanation.graph);
        let receipt = explanation
            .entry
            .as_ref()
            .map(|entry| entry.receipt.clone())
            .filter(|receipt| !receipt.is_empty())
            .unwrap_or_else(|| "not recorded".to_string());

        Ok(Self {
            query: explanation.query,
            package,
            available: explanation.entry.is_some(),
            requesting,
            origin,
            trust,
            disk,
            dependents,
            receipt,
        })
    }

    pub fn to_json(&self) -> String {
        let bytes = self
            .disk
            .bytes
            .map(|bytes| bytes.to_string())
            .unwrap_or_else(|| "null".to_string());
        jet_foundation::Report::render_status_json(
            "ok",
            true,
            "why",
            &format!(
                ",\"query\":{},\"package\":{},\"available\":{},\"requesting\":{{\"env_file\":{},\"lock_file\":{}}},\"origin\":{{\"catalog\":{},\"endpoint\":{},\"cache_endpoint\":{},\"signature_chain\":{},\"source\":{},\"source_digest\":{}}},\"trust\":{{\"grade\":{},\"reason\":{}}},\"disk\":{{\"bytes\":{},\"objects\":{}}},\"dependents\":{},\"receipt\":{}",
                JSON::quote(&self.query),
                JSON::quote(&self.package),
                self.available,
                self.requesting.env_file.to_json(),
                self.requesting.lock_file.to_json(),
                JSON::quote(&self.origin.catalog),
                JSON::quote(&self.origin.endpoint),
                JSON::quote(&self.origin.cache_endpoint),
                JSON::quote(&self.origin.signature_chain),
                JSON::quote(&self.origin.source),
                JSON::quote(&self.origin.source_digest),
                JSON::quote(&self.trust.grade),
                JSON::quote(&self.trust.reason),
                bytes,
                self.disk.objects,
                json_array(self.dependents.iter().map(|dependent| JSON::quote(dependent))),
                JSON::quote(&self.receipt),
            ),
        )
    }

    pub fn text(&self) -> String {
        let dependents = if self.dependents.is_empty() {
            "-".to_string()
        } else {
            self.dependents.join(", ")
        };
        let disk = match self.disk.bytes {
            Some(bytes) => format!("{bytes} B ({} closure objects)", self.disk.objects),
            None if self.disk.objects > 0 => {
                format!("not recorded ({} closure objects)", self.disk.objects)
            }
            None => "not recorded".to_string(),
        };
        let mut output = String::new();
        output.push_str(&format!("package why {}\n", self.package));
        if !self.available {
            output.push_str("status       no realized StoreEntry\n");
        }
        output.push_str(&format!(
            "requesting\n  env file     {}\n  lock line    {}\n",
            location_text(&self.requesting.env_file),
            location_text(&self.requesting.lock_file),
        ));
        output.push_str(&format!(
            "origin\n  catalog      {}\n  endpoint     {}\n  cache        {}\n  signature chain {}\n  source       {}\n  source digest {}\n",
            empty_dash(&self.origin.catalog),
            empty_dash(&self.origin.endpoint),
            empty_dash(&self.origin.cache_endpoint),
            empty_dash(&self.origin.signature_chain),
            empty_dash(&self.origin.source),
            empty_dash(&self.origin.source_digest),
        ));
        output.push_str(&format!(
            "trust        {} ({})\ndisk         {}\ndependents   {}\nreceipt      {}\n",
            self.trust.grade,
            self.trust.reason,
            disk,
            dependents,
            empty_dash(&self.receipt),
        ));
        output
    }
}

impl WhyLocation {
    fn to_json(&self) -> String {
        format!(
            "{{\"path\":{},\"line\":{},\"text\":{}}}",
            JSON::quote(&self.path),
            self.line
                .map(|line| line.to_string())
                .unwrap_or_else(|| "null".to_string()),
            self.text
                .as_deref()
                .map(JSON::quote)
                .unwrap_or_else(|| "null".to_string()),
        )
    }
}

fn requesting_projection(names: &[String], qualified: &[String]) -> std::io::Result<WhyRequesting> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let env_path = nearest_real_file(&cwd, Syntax::ENV_FILE);
    let env_file = source_location(env_path, names, qualified, false);
    let lock_path = super::nearest_lock_path(&cwd)?;
    let lock_file = source_location(lock_path, names, qualified, true);
    Ok(WhyRequesting {
        env_file,
        lock_file,
    })
}

fn nearest_real_file(start: &Path, name: &str) -> Option<PathBuf> {
    let mut directory = Some(start);
    while let Some(current) = directory {
        let candidate = current.join(name);
        if let Ok(metadata) = fs::symlink_metadata(&candidate) {
            if metadata.is_file() && !metadata.file_type().is_symlink() {
                return Some(candidate);
            }
        }
        directory = current.parent();
    }
    None
}

fn source_location(
    path: Option<PathBuf>,
    names: &[String],
    qualified: &[String],
    lock: bool,
) -> WhyLocation {
    let Some(path) = path else {
        return WhyLocation {
            path: "not found".to_string(),
            line: None,
            text: None,
        };
    };
    let path_text = path.display().to_string();
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => {
            return WhyLocation {
                path: path_text,
                line: None,
                text: None,
            }
        }
    };
    let found = if lock {
        lock_line(&raw, names, qualified)
    } else {
        raw.lines()
            .enumerate()
            .find(|(_, line)| source_line_matches(line, names, qualified))
            .map(|(index, line)| (index + 1, line.trim().to_string()))
    };
    WhyLocation {
        path: path_text,
        line: found.as_ref().map(|(line, _)| *line),
        text: found.map(|(_, text)| text),
    }
}

fn lock_line(raw: &str, names: &[String], qualified: &[String]) -> Option<(usize, String)> {
    let mut block: Option<Vec<(usize, &str)>> = None;
    for (index, line) in raw.lines().enumerate() {
        if line.trim() == "[[package]]" {
            if let Some(previous) = block.take() {
                if let Some(found) = matching_lock_block(&previous, names, qualified) {
                    return Some(found);
                }
            }
            block = Some(Vec::new());
        }
        if let Some(block) = block.as_mut() {
            block.push((index + 1, line));
        }
    }
    block
        .as_deref()
        .and_then(|block| matching_lock_block(block, names, qualified))
}

fn matching_lock_block(
    block: &[(usize, &str)],
    names: &[String],
    qualified: &[String],
) -> Option<(usize, String)> {
    let name_line = block.iter().find(|(_, line)| {
        names.iter().any(|name| {
            !name.is_empty() && lock_name(line) == Some(name.as_str())
        })
    });
    if let Some((line, text)) = name_line {
        return Some((*line, text.trim().to_string()));
    }
    block
        .iter()
        .find(|(_, line)| {
            qualified
                .iter()
                .any(|reference| !reference.is_empty() && line.contains(reference))
        })
        .map(|(line, text)| (*line, text.trim().to_string()))
}

fn source_line_matches(line: &str, names: &[String], qualified: &[String]) -> bool {
    let code = line.split_once("//").map_or(line, |(code, _)| code);
    names
        .iter()
        .chain(qualified.iter())
        .any(|value| !value.is_empty() && contains_package_token(code, value))
}

fn contains_package_token(haystack: &str, needle: &str) -> bool {
    let mut offset = 0;
    while let Some(relative) = haystack[offset..].find(needle) {
        let start = offset + relative;
        let end = start + needle.len();
        let before = haystack[..start].chars().next_back();
        let after = haystack[end..].chars().next();
        if !before.is_some_and(package_token_char) && !after.is_some_and(package_token_char) {
            return true;
        }
        offset = end;
    }
    false
}

fn package_token_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
}

fn lock_name(line: &str) -> Option<&str> {
    let line = line.trim();
    let value = line.strip_prefix("name = \"")?;
    value.strip_suffix('\"')
}

fn location_text(location: &WhyLocation) -> String {
    match location.line {
        Some(line) => match location.text.as_deref() {
            Some(text) => format!("{}:{line} ({})", location.path, compact_line(text)),
            None => format!("{}:{line}", location.path),
        },
        None => location.path.clone(),
    }
}

fn compact_line(line: &str) -> String {
    const MAX_CHARS: usize = 120;
    let mut compact = line.chars().take(MAX_CHARS).collect::<String>();
    if line.chars().count() > MAX_CHARS {
        compact.push_str("…");
    }
    compact
}

fn origin_projection(
    roots: &Roots,
    provider: &str,
    facts: Option<&BTreeMap<String, String>>,
    source: &str,
    source_digest: &str,
) -> WhyOrigin {
    let fact = |key: &str| facts.and_then(|facts| facts.get(key)).cloned();
    let tier = fact("nix.index.tier");
    let catalog = catalog_origin(tier.as_deref(), facts, provider);
    let cache_endpoint = fact("nix.cache.endpoint")
        .or_else(|| {
            (provider == "nix")
                .then(|| config_value(roots, "config/nix-cache-v1.endpoint"))
                .flatten()
        })
        .or_else(|| {
            (provider == "nix").then(|| "https://cache.nixos.org".to_string())
        })
        .or_else(|| fact("source.url"))
        .unwrap_or_else(|| "not recorded".to_string());
    // A signed-index endpoint is the strongest catalog origin. If it was not
    // persisted, the configured cache endpoint still answers where the bytes
    // came from for a realized Nix package.
    let endpoint = fact("nix.index.endpoint")
        .or_else(|| {
            (provider == "nix")
                .then(|| config_value(roots, "config/nix-index-v1.endpoint"))
                .flatten()
        })
        .or_else(|| {
            if provider == "nix" && cache_endpoint != "not recorded" {
                Some(cache_endpoint.clone())
            } else {
                fact("source.url")
            }
        })
        .unwrap_or_else(|| "not recorded".to_string());
    let signature_chain = fact("nix.index.signature-chain")
        .or_else(|| {
            fact("artifact.verification").map(|verification| {
                format!("not applicable ({verification})")
            })
        })
        .unwrap_or_else(|| "not recorded".to_string());
    WhyOrigin {
        catalog,
        endpoint,
        cache_endpoint,
        signature_chain,
        source: if source.is_empty() {
            "not recorded".to_string()
        } else {
            source.to_string()
        },
        source_digest: if source_digest.is_empty() {
            "not recorded".to_string()
        } else {
            source_digest.to_string()
        },
    }
}

fn catalog_origin(
    tier: Option<&str>,
    facts: Option<&BTreeMap<String, String>>,
    provider: &str,
) -> String {
    let Some(tier) = tier else {
        return facts
            .and_then(|facts| {
                facts
                    .get("source.repository")
                    .or_else(|| facts.get("source.kind"))
            })
            .cloned()
            .or_else(|| (!provider.is_empty()).then(|| provider.to_string()))
            .unwrap_or_else(|| "not recorded".to_string());
    };
    let Some(proof) = facts.and_then(|facts| facts.get("nix.index.proof.v1")) else {
        return tier.to_string();
    };
    let channel = json_string_field(proof, "channel");
    let revision = json_string_field(proof, "revision");
    let system = json_string_field(proof, "system");
    match (channel, revision, system) {
        (Some(channel), Some(revision), Some(system)) => {
            format!("{tier} ({channel}@{revision} / {system})")
        }
        _ => tier.to_string(),
    }
}

fn json_string_field(raw: &str, field: &str) -> Option<String> {
    let value = JSON::parse(raw).ok()?;
    let object = value.as_object().ok()?;
    object.get(field)?.as_str().ok().map(str::to_string)
}

fn config_value(roots: &Roots, relative: &str) -> Option<String> {
    let path = roots.root.join(relative);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    let value = fs::read_to_string(path).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn trust_projection(
    provider: &str,
    facts: Option<&BTreeMap<String, String>>,
) -> WhyTrust {
    let fact = |key: &str| facts.and_then(|facts| facts.get(key)).map(String::as_str);
    let tier = fact("nix.index.tier");
    let source_kind = fact("source.kind");
    let local_mapping = tier == Some("local-unofficial")
        || source_kind == Some("local-unofficial-catalog")
        || (provider == "nix" && fact("nix.index.trust") == Some("unverified"));
    if local_mapping {
        let native_recipe = provider == "jetpackage"
            && source_kind == Some("local-unofficial-catalog");
        return WhyTrust {
            grade: "unverified-mapping".to_string(),
            reason: if native_recipe {
                "native recipe mapping is unverified; artifact bytes remain SHA-256-verified"
                    .to_string()
            } else {
                "name-to-store-path mapping is unverified; Nix cache bytes remain signature-verified"
                    .to_string()
            },
        };
    }
    if (tier == Some("official-signed") && fact("nix.index.signature-chain") == Some("present"))
        || (fact("nix.index.trust") == Some("verified")
            && fact("nix.index.signature-chain") == Some("present"))
    {
        return WhyTrust {
            grade: "signed".to_string(),
            reason: "the catalog and signature chain are verified".to_string(),
        };
    }
    if fact("cache.reproducibility").is_some_and(reproducibility_proof) {
        return WhyTrust {
            grade: "reproduced".to_string(),
            reason: "the producer record carries a reproducibility proof".to_string(),
        };
    }
    WhyTrust {
        grade: "unverified-mapping".to_string(),
        reason: "no signed mapping or reproducibility proof is recorded".to_string(),
    }
}

fn reproducibility_proof(value: &str) -> bool {
    value == "attested-v1" || value.starts_with("independent-agreeing-v1:")
}

fn disk_projection(graph: &ExplainGraph) -> WhyDisk {
    let mut paths = BTreeSet::new();
    let mut bytes = 0u64;
    let mut objects = 0usize;
    let mut measurable = true;
    for object in &graph.closure {
        if object.external || !object.known || object.path.is_empty() {
            continue;
        }
        if !paths.insert(object.path.clone()) {
            continue;
        }
        let path = Path::new(&object.path);
        objects += 1;
        let Some(size) = disk_size(path) else {
            measurable = false;
            continue;
        };
        let Some(next) = bytes.checked_add(size) else {
            measurable = false;
            continue;
        };
        bytes = next;
    }
    WhyDisk {
        bytes: (objects > 0 && measurable).then_some(bytes),
        objects,
    }
}

fn disk_size(path: &Path) -> Option<u64> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() {
        return None;
    }
    if metadata.is_dir() {
        Some(super::dir_size(path))
    } else if metadata.is_file() {
        Some(metadata.len())
    } else {
        None
    }
}

fn dependent_projection(graph: &ExplainGraph) -> Vec<String> {
    graph
        .referrers
        .iter()
        .chain(graph.direct_referrers.iter())
        .flat_map(|object| object.owners.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn find_entry(entries: &[StoreEntry], query: &str) -> std::io::Result<Option<StoreEntry>> {
    let exact: Vec<_> = entries
        .iter()
        .filter(|entry| entry.id == query || entry.reference == query)
        .cloned()
        .collect();
    if exact.len() > 1 {
        return Err(ambiguous_query(query, &exact));
    }
    if let Some(entry) = exact.into_iter().next() {
        return Ok(Some(entry));
    }
    let package = package_name(query);
    let mut matches: Vec<_> = entries
        .iter()
        .filter(|entry| entry.name == package || package_name(&entry.reference) == package)
        .cloned()
        .collect();
    matches.sort_by(|left, right| left.id.cmp(&right.id));
    if matches.len() > 1 {
        return Err(ambiguous_query(query, &matches));
    }
    Ok(matches.into_iter().next())
}

fn ambiguous_query(query: &str, entries: &[StoreEntry]) -> std::io::Error {
    let matches = entries
        .iter()
        .map(|entry| entry.reference.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    std::io::Error::other(format!(
        "package explain query `{query}` is ambiguous; matching references: {matches}"
    ))
}

fn package_name(query: &str) -> String {
    let target = ProviderFacts::for_reference("", query).target;
    if target != query {
        return target;
    }
    query
        .split_once(':')
        .map(|(_, value)| value)
        .or_else(|| query.split_once('.').map(|(_, value)| value))
        .unwrap_or(query)
        .to_string()
}

fn entry_projection(entry: &StoreEntry) -> ExplainEntry {
    ExplainEntry {
        id: entry.id.clone(),
        name: entry.name.clone(),
        version: entry.version.clone(),
        reference: entry.reference.clone(),
        output_hash: entry.envelope.output_hash.clone(),
        output_path: entry.out.clone(),
        bin_path: entry.bin.clone(),
        rlib_path: entry.rlib.clone(),
        receipt: entry.receipt.clone(),
        realized_at: entry.realized_at,
        last_used_at: entry.last_used_at,
    }
}

fn load_graph(roots: &Roots, reports: &mut Vec<ExplainReport>) -> Option<ClosureGraph> {
    match super::Closure::closure_graph_read_only(roots) {
        Ok(graph) => Some(graph),
        Err(error) => {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!("closure proof is unavailable: {error}"),
            });
            match super::Closure::closure_graph_structure_read_only(roots) {
                Ok(graph) => Some(graph),
                Err(error) => {
                    reports.push(ExplainReport {
                        kind: "loss".to_string(),
                        message: format!("closure structure is unavailable: {error}"),
                    });
                    None
                }
            }
        }
    }
}

/// Check the immutable receipt projection without repairing it. Explain must
/// identify a missing, unsafe, or mismatched receipt as a loss of proof while
/// leaving recovery to `hangar recover` or an explicit repair operation.
fn receipt_projection(
    roots: &Roots,
    entry: &StoreEntry,
    graph: Option<&ClosureGraph>,
    reports: &mut Vec<ExplainReport>,
) -> bool {
    let before = reports.len();
    if entry.receipt.is_empty() {
        reports.push(ExplainReport {
            kind: "loss".to_string(),
            message: format!("StoreEntry `{}` has no connected Hangar receipt", entry.id),
        });
        return false;
    }
    if !super::valid_receipt_digest(&entry.receipt) {
        reports.push(ExplainReport {
            kind: "conflict".to_string(),
            message: format!(
                "StoreEntry `{}` has an invalid Hangar receipt digest `{}`",
                entry.id, entry.receipt
            ),
        });
        return false;
    }
    let path = roots
        .hangar_dir()
        .join(super::Closure::RECEIPTS_DIR)
        .join(&entry.receipt);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!("Hangar receipt `{}` is not a regular file", path.display()),
            });
        }
        Ok(_) => match fs::read(&path) {
            Ok(bytes) => {
                let actual = format!("sha256-{}", crate::SHA256::sha256_hex(&bytes));
                if actual != entry.receipt {
                    reports.push(ExplainReport {
                        kind: "conflict".to_string(),
                        message: format!(
                            "Hangar receipt `{}` is corrupt: content hashes as `{actual}`",
                            entry.receipt
                        ),
                    });
                }
            }
            Err(error) => reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!(
                    "Hangar receipt `{}` could not be read: {error}",
                    entry.receipt
                ),
            }),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => reports.push(ExplainReport {
            kind: "loss".to_string(),
            message: format!("Hangar receipt `{}` is missing", entry.receipt),
        }),
        Err(error) => reports.push(ExplainReport {
            kind: "loss".to_string(),
            message: format!(
                "Hangar receipt `{}` could not be inspected: {error}",
                entry.receipt
            ),
        }),
    }
    if let Some(graph) = graph {
        if let Some(record) = graph.records.get(&entry.id) {
            match super::parse_meta(&record.package_meta) {
                Some(meta)
                    if meta.receipt != entry.receipt
                        || meta.name != entry.name
                        || meta.version != entry.version
                        || meta.reference != entry.reference
                        || meta.envelope.output_hash != entry.envelope.output_hash =>
                {
                    reports.push(ExplainReport {
                        kind: "conflict".to_string(),
                        message: format!(
                            "Hangar closure record `{}` disagrees with the Store receipt projection",
                            entry.id
                        ),
                    });
                }
                Some(_) | None => {}
            }
        }
    }
    reports.len() == before
}

fn provider_projection(
    entry: &StoreEntry,
    reports: &mut Vec<ExplainReport>,
) -> Option<ExplainProvider> {
    let producer = match ProducerRecord::decode(&entry.producer_record) {
        Ok(producer) => producer,
        Err(error) => {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!("producer provenance is unavailable: {error}"),
            });
            return None;
        }
    };
    let facts = match producer.facts.get("provider-facts") {
        Some(raw) => match ProviderFacts::from_json(raw) {
            Ok(facts) => {
                if let Err(error) = facts.validate() {
                    reports.push(ExplainReport {
                        kind: "loss".to_string(),
                        message: format!("embedded provider facts fail validation: {error}"),
                    });
                }
                Some(facts)
            }
            Err(error) => {
                reports.push(ExplainReport {
                    kind: "loss".to_string(),
                    message: format!("embedded provider facts could not be decoded: {error}"),
                });
                None
            }
        },
        None => {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: "producer record lacks the shared provider-facts carrier".to_string(),
            });
            None
        }
    };
    if let Some(facts) = facts.as_ref() {
        let entry_facts = ProviderFacts::for_reference("", &entry.reference);
        let canonical_entry = entry_facts.qualified_reference();
        let unpinned_alias_matches = entry_facts.selector.raw.is_empty()
            && facts.reference == entry.reference
            && facts.target == entry_facts.target
            && (facts.provider == entry_facts.provider
                || matches!(
                    facts.facts.get("provider.authority"),
                    Some(crate::ProviderFactValue::Text(authority))
                        if authority == &entry_facts.provider
                ));
        if facts.qualified_reference() != canonical_entry && !unpinned_alias_matches {
            reports.push(ExplainReport {
                kind: "conflict".to_string(),
                message: format!(
                    "embedded provider facts identify `{}` but the Store entry is `{}`",
                    facts.qualified_reference(),
                    canonical_entry
                ),
            });
        }
        if !producer.provider.is_empty() && producer.provider != facts.provider {
            reports.push(ExplainReport {
                kind: "conflict".to_string(),
                message: format!(
                    "embedded provider facts name `{}` but the producer names `{}`",
                    facts.provider, producer.provider
                ),
            });
        }
        if !producer.immutable_source.is_empty()
            && !facts.resolved_source.is_empty()
            && producer.immutable_source != facts.resolved_source
        {
            reports.push(ExplainReport {
                kind: "conflict".to_string(),
                message: format!(
                    "embedded provider source `{}` differs from producer source `{}`",
                    facts.resolved_source, producer.immutable_source
                ),
            });
        }
        match producer.facts.get("provider-facts-digest") {
            Some(expected) => {
                let actual = facts.digest();
                if expected != &actual {
                    reports.push(ExplainReport {
                        kind: "conflict".to_string(),
                        message: format!(
                            "embedded provider-facts digest differs from its producer record: `{expected}` vs `{actual}`"
                        ),
                    });
                }
            }
            None => {
                reports.push(ExplainReport {
                    kind: "loss".to_string(),
                    message: "producer record has provider facts but no provider-facts digest"
                        .to_string(),
                });
            }
        }
        report_provider_facts(facts, reports);
        report_provider_validation(facts, "producer", reports);
    }
    Some(ExplainProvider {
        provider: producer.provider,
        immutable_source: producer.immutable_source,
        source_digest: producer.source_digest,
        toolchain_facts: producer.toolchain_facts,
        policy_facts: producer.policy_facts,
        producer_facts: producer.facts,
        provider_facts: facts,
        locked_provider_facts: None,
        profile_facts: Vec::new(),
        native_document: entry.producer_record.clone(),
    })
}

fn report_provider_facts(facts: &ProviderFacts, reports: &mut Vec<ExplainReport>) {
    for loss in &facts.losses {
        reports.push(ExplainReport {
            kind: "loss".to_string(),
            message: format!(
                "provider fact `{}` is lossy: {} ({})",
                loss.key, loss.reason, loss.source
            ),
        });
    }
    for conflict in &facts.conflicts {
        reports.push(ExplainReport {
            kind: "conflict".to_string(),
            message: format!(
                "provider fact `{}` conflicts: {} vs {} ({})",
                conflict.key, conflict.left, conflict.right, conflict.source
            ),
        });
    }
}

fn report_provider_validation(
    facts: &ProviderFacts,
    context: &str,
    reports: &mut Vec<ExplainReport>,
) {
    if let Err(error) = facts.validate() {
        if facts.losses.is_empty() && facts.conflicts.is_empty() {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!("{context} provider facts fail validation: {error}"),
            });
        }
    }
}

fn locked_provider_facts(
    entry: &StoreEntry,
    reports: &mut Vec<ExplainReport>,
) -> Option<ProviderFacts> {
    let cwd = std::env::current_dir().ok()?;
    let lock_path = match super::nearest_lock_path(&cwd) {
        Ok(Some(path)) => path,
        Ok(None) => return None,
        Err(error) => {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!("project lock path could not be inspected: {error}"),
            });
            return None;
        }
    };
    let project = lock_path.parent()?;
    let lock = match SemanticLock::load(project) {
        Some(lock) => lock,
        None => {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!(
                    "project lock `{}` is present but could not be read",
                    lock_path.display()
                ),
            });
            return None;
        }
    };
    let mut matches = Vec::new();
    let canonical_entry = ProviderFacts::for_reference("", &entry.reference).qualified_reference();
    for record in lock.records {
        let record_matches = lock_record_matches_entry(&record, entry, &canonical_entry);
        let Some(raw) = record.future_fields.get("provider-facts") else {
            if record_matches {
                reports.push(ExplainReport {
                    kind: "loss".to_string(),
                    message: format!(
                        "locked provider record `{}` lacks its full provider-facts carrier",
                        record.identity.exact
                    ),
                });
            }
            continue;
        };
        match ProviderFacts::from_json(raw) {
            Ok(facts) => {
                let facts_match = facts.reference == entry.reference
                    || facts.qualified_reference() == canonical_entry
                    || record.identity.exact == entry.reference
                    || record.identity.exact == facts.qualified_reference();
                let matches_entry = record_matches || facts_match;
                if matches_entry {
                    report_provider_facts(&facts, reports);
                    report_provider_validation(&facts, "locked", reports);
                }
                let mut record_identity_ok = true;
                match record.future_fields.get("provider-facts-digest") {
                    Some(expected) => {
                        let actual = facts.digest();
                        if matches_entry && expected != &actual {
                            record_identity_ok = false;
                            reports.push(ExplainReport {
                                kind: "conflict".to_string(),
                                message: format!(
                                    "locked provider-fact digest disagrees with its record: {expected} vs {actual}"
                                ),
                            });
                        }
                    }
                    None if matches_entry => {
                        record_identity_ok = false;
                        reports.push(ExplainReport {
                            kind: "loss".to_string(),
                            message: format!(
                                "locked provider facts have no digest: {}",
                                record.identity.exact
                            ),
                        });
                    }
                    None => {}
                }
                if matches_entry && record.identity.hash.is_empty() {
                    record_identity_ok = false;
                    reports.push(ExplainReport {
                        kind: "loss".to_string(),
                        message: format!(
                            "locked provider record `{}` lacks its fact identity hash",
                            record.identity.exact
                        ),
                    });
                } else if matches_entry && record.identity.hash != facts.digest() {
                    record_identity_ok = false;
                    reports.push(ExplainReport {
                        kind: "conflict".to_string(),
                        message: format!(
                            "locked provider identity hash disagrees for {}: {} vs {}",
                            record.identity.exact,
                            record.identity.hash,
                            facts.digest()
                        ),
                    });
                }
                if facts.validate().is_err() {
                    continue;
                }
                if matches_entry && record_identity_ok {
                    matches.push(facts);
                }
            }
            Err(error) if record_matches => reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!(
                    "locked provider facts for {} could not be decoded: {error}",
                    record.identity.exact
                ),
            }),
            Err(_) => {}
        }
    }
    matches.sort_by(|left, right| left.digest().cmp(&right.digest()));
    matches.dedup_by(|left, right| left.digest() == right.digest());
    if matches.len() > 1 {
        reports.push(ExplainReport {
            kind: "conflict".to_string(),
            message: format!(
                "multiple locked provider-fact records match `{}`: {}",
                entry.reference,
                matches
                    .iter()
                    .map(ProviderFacts::digest)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    matches.into_iter().next()
}

fn lock_record_matches_entry(
    record: &SemanticLock::SemanticRecord,
    entry: &StoreEntry,
    canonical_entry: &str,
) -> bool {
    record.identity.kind.as_str() == "package"
        && (record.identity.exact == entry.reference
            || record.identity.exact == canonical_entry
            || record.identity.key == format!("provider:{canonical_entry}")
            || record.identity.key == format!("provider:{}", entry.reference))
}

fn profile_provider_facts(
    entry: &StoreEntry,
    reports: &mut Vec<ExplainReport>,
) -> Vec<ExplainProfile> {
    let Some(generations) = nearest_profile_generations(reports) else {
        return Vec::new();
    };
    let mut profiles = Vec::new();
    let Ok(profile_dirs) = fs::read_dir(&generations) else {
        reports.push(ExplainReport {
            kind: "loss".to_string(),
            message: format!(
                "profile provider facts are unavailable: could not read {}",
                generations.display()
            ),
        });
        return profiles;
    };
    for profile in profile_dirs {
        let profile = match profile {
            Ok(profile) => profile,
            Err(error) => {
                reports.push(ExplainReport {
                    kind: "loss".to_string(),
                    message: format!(
                        "profile provider facts are unavailable while reading {}: {error}",
                        generations.display()
                    ),
                });
                continue;
            }
        };
        let profile_path = profile.path();
        let profile_metadata = match fs::symlink_metadata(&profile_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                reports.push(ExplainReport {
                    kind: "loss".to_string(),
                    message: format!(
                        "profile provider facts are unavailable for {}: {error}",
                        profile_path.display()
                    ),
                });
                continue;
            }
        };
        if profile_metadata.file_type().is_symlink() {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!(
                    "profile provider facts are unavailable: {} is a symlink",
                    profile_path.display()
                ),
            });
            continue;
        }
        if !profile_metadata.is_dir() {
            continue;
        }
        let profile_name = profile.file_name().to_string_lossy().into_owned();
        let generation_dir = profile_path.join("generations");
        let generations = match fs::read_dir(&generation_dir) {
            Ok(generations) => generations,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                reports.push(ExplainReport {
                    kind: "loss".to_string(),
                    message: format!(
                        "profile {profile_name} provider facts are unavailable: could not read {}: {error}",
                        generation_dir.display()
                    ),
                });
                continue;
            }
        };
        for generation in generations {
            let generation = match generation {
                Ok(generation) => generation,
                Err(error) => {
                    reports.push(ExplainReport {
                        kind: "loss".to_string(),
                        message: format!(
                            "profile {profile_name} provider facts are unavailable while reading {}: {error}",
                            generation_dir.display()
                        ),
                    });
                    continue;
                }
            };
            let generation_path = generation.path();
            let generation_metadata = match fs::symlink_metadata(&generation_path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    reports.push(ExplainReport {
                        kind: "loss".to_string(),
                        message: format!(
                            "profile {profile_name} generation provider facts are unavailable for {}: {error}",
                            generation_path.display()
                        ),
                    });
                    continue;
                }
            };
            if generation_metadata.file_type().is_symlink() {
                reports.push(ExplainReport {
                    kind: "loss".to_string(),
                    message: format!(
                        "profile {profile_name} contains a symlinked generation `{}`",
                        generation.file_name().to_string_lossy()
                    ),
                });
                continue;
            }
            if !generation_metadata.is_dir() {
                continue;
            }
            let Ok(number) = generation.file_name().to_string_lossy().parse::<u64>() else {
                continue;
            };
            let metadata_path = generation_path.join("meta.json");
            let raw = match fs::read_to_string(&metadata_path) {
                Ok(raw) => raw,
                Err(error) => {
                    reports.push(ExplainReport {
                        kind: "loss".to_string(),
                        message: format!(
                            "profile {profile_name} generation {number} metadata is unavailable: {error}"
                        ),
                    });
                    continue;
                }
            };
            let value = match JSON::parse(&raw) {
                Ok(value) => value,
                Err(error) => {
                    reports.push(ExplainReport {
                        kind: "loss".to_string(),
                        message: format!(
                            "profile {profile_name} generation {number} metadata is invalid: {error}"
                        ),
                    });
                    continue;
                }
            };
            let object = match value.as_object() {
                Ok(object) => object,
                Err(error) => {
                    reports.push(ExplainReport {
                        kind: "loss".to_string(),
                        message: format!(
                            "profile {profile_name} generation {number} metadata is not an object: {error}"
                        ),
                    });
                    continue;
                }
            };
            if object.get("schema").and_then(|value| value.as_str().ok())
                != Some("jet-package-generation-v1")
            {
                reports.push(ExplainReport {
                    kind: "loss".to_string(),
                    message: format!(
                        "profile {profile_name} generation {number} has unsupported metadata schema"
                    ),
                });
                continue;
            }
            let Some(crate::JSON::JSONValue::Array(packages)) = object.get("packages") else {
                reports.push(ExplainReport {
                    kind: "loss".to_string(),
                    message: format!(
                        "profile {profile_name} generation {number} metadata lacks packages"
                    ),
                });
                continue;
            };
            for package in packages {
                let Ok(package) = package.as_object() else {
                    reports.push(ExplainReport {
                        kind: "loss".to_string(),
                        message: format!(
                            "profile {profile_name} generation {number} contains a non-object package"
                        ),
                    });
                    continue;
                };
                let output_hash = package
                    .get("output_hash")
                    .and_then(|value| value.as_str().ok())
                    .unwrap_or_default();
                let raw_reference = package
                    .get("raw")
                    .and_then(|value| value.as_str().ok())
                    .unwrap_or_default();
                let target = package
                    .get("target")
                    .and_then(|value| value.as_str().ok())
                    .unwrap_or_default();
                if output_hash != entry.envelope.output_hash
                    && raw_reference != entry.reference
                    && target != entry.name
                {
                    continue;
                }
                if output_hash != entry.envelope.output_hash {
                    reports.push(ExplainReport {
                        kind: "loss".to_string(),
                        message: format!(
                            "profile {profile_name} generation {number} package identity does not retain the current output hash"
                        ),
                    });
                }
                let Some(facts_value) = package.get("provider_facts") else {
                    reports.push(ExplainReport {
                        kind: "loss".to_string(),
                        message: format!(
                            "profile {profile_name} generation {number} package {} lacks provider facts",
                            entry.reference
                        ),
                    });
                    continue;
                };
                let facts = match ProviderFacts::from_json_value(facts_value) {
                    Ok(facts) => facts,
                    Err(error) => {
                        reports.push(ExplainReport {
                            kind: "loss".to_string(),
                            message: format!(
                                "profile {profile_name} generation {number} provider facts are invalid: {error}"
                            ),
                        });
                        continue;
                    }
                };
                report_provider_facts(&facts, reports);
                report_provider_validation(&facts, "profile", reports);
                for (field, expected, actual) in [
                    ("reference", raw_reference, facts.reference.as_str()),
                    ("target", target, facts.target.as_str()),
                    (
                        "provider",
                        package
                            .get("provider")
                            .and_then(|value| value.as_str().ok())
                            .unwrap_or_default(),
                        facts.provider.as_str(),
                    ),
                ] {
                    if !expected.is_empty() && expected != actual {
                        reports.push(ExplainReport {
                            kind: "conflict".to_string(),
                            message: format!(
                                "profile {profile_name} generation {number} {field} differs: {expected} vs {actual}"
                            ),
                        });
                    }
                }
                if let Some(expected) = package
                    .get("provider_facts_digest")
                    .and_then(|value| value.as_str().ok())
                {
                    let actual = facts.digest();
                    if expected != actual {
                        reports.push(ExplainReport {
                            kind: "conflict".to_string(),
                            message: format!(
                                "profile {profile_name} generation {number} provider-fact digest disagrees: {expected} vs {actual}"
                            ),
                        });
                    }
                } else {
                    reports.push(ExplainReport {
                        kind: "loss".to_string(),
                        message: format!(
                            "profile {profile_name} generation {number} package {} lacks provider-fact digest",
                            entry.reference
                        ),
                    });
                }
                profiles.push(ExplainProfile {
                    profile: profile_name.clone(),
                    generation: number,
                    output_hash: output_hash.to_string(),
                    provider_facts: facts,
                });
            }
        }
    }
    profiles.sort_by(|left, right| {
        (&left.profile, left.generation).cmp(&(&right.profile, right.generation))
    });
    profiles
}

fn nearest_profile_generations(reports: &mut Vec<ExplainReport>) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let mut directory = Some(cwd.as_path());
    while let Some(current) = directory {
        let profiles = super::managed_dir(current).join("profiles");
        match fs::symlink_metadata(&profiles) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                reports.push(ExplainReport {
                    kind: "loss".to_string(),
                    message: format!("profile state {} is a symlink", profiles.display()),
                });
                return None;
            }
            Ok(metadata) if metadata.is_dir() => return Some(profiles),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                reports.push(ExplainReport {
                    kind: "loss".to_string(),
                    message: format!(
                        "profile state {} is unavailable: {error}",
                        profiles.display()
                    ),
                });
                return None;
            }
        }
        directory = current.parent();
    }
    None
}

fn compare_provider_identity(
    source: &ProviderFacts,
    locked: &ProviderFacts,
    reports: &mut Vec<ExplainReport>,
) {
    for (field, left, right) in [
        ("provider", &source.provider, &locked.provider),
        ("reference", &source.reference, &locked.reference),
        ("target", &source.target, &locked.target),
        (
            "resolved_source",
            &source.resolved_source,
            &locked.resolved_source,
        ),
    ] {
        if !left.is_empty() && !right.is_empty() && left != right {
            reports.push(ExplainReport {
                kind: "conflict".to_string(),
                message: format!(
                    "provider identity field `{field}` differs between producer and lock: `{left}` vs `{right}`"
                ),
            });
        }
    }
    let source_ref = source.qualified_reference();
    let locked_ref = locked.qualified_reference();
    if source_ref != locked_ref {
        reports.push(ExplainReport {
            kind: "conflict".to_string(),
            message: format!(
                "provider selector differs between producer and lock: `{source_ref}` vs `{locked_ref}`"
            ),
        });
    }
    let source_digest = source.digest();
    let locked_digest = locked.digest();
    if source_digest != locked_digest {
        reports.push(ExplainReport {
            kind: "conflict".to_string(),
            message: format!(
                "provider fact digest differs between producer and lock: `{source_digest}` vs `{locked_digest}`"
            ),
        });
    }
}

fn graph_projection(
    entries: &[StoreEntry],
    entry: &StoreEntry,
    graph: Option<&ClosureGraph>,
    reports: &mut Vec<ExplainReport>,
) -> ExplainGraph {
    let Some(graph) = graph else {
        return ExplainGraph::default();
    };
    let digest = &entry.envelope.output_hash;
    let record_exists = graph.records.values().any(|record| {
        record.id == entry.id || record.outputs.values().any(|output| output == digest)
    });
    if !record_exists {
        reports.push(ExplainReport {
            kind: "loss".to_string(),
            message: format!(
                "closure graph has no record for StoreEntry `{}` / output `{digest}`",
                entry.id
            ),
        });
    }
    let direct = graph.direct_references(digest);
    let transitive = graph.transitive_references(digest);
    let direct_referrers = graph.referrers(digest);
    let referrers = graph.transitive_referrers(digest);
    let closure = graph.closure(digest);
    for value in direct
        .iter()
        .chain(transitive.iter())
        .chain(direct_referrers.iter())
        .chain(referrers.iter())
        .chain(closure.iter())
    {
        if !graph.objects.contains_key(value) && value != digest {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!("closure edge references unknown object `{value}`"),
            });
        }
    }
    ExplainGraph {
        direct_dependencies: direct
            .iter()
            .map(|value| object_projection(entries, graph, value))
            .collect(),
        dependencies: transitive
            .iter()
            .map(|value| object_projection(entries, graph, value))
            .collect(),
        direct_referrers: direct_referrers
            .iter()
            .map(|value| object_projection(entries, graph, value))
            .collect(),
        referrers: referrers
            .iter()
            .map(|value| object_projection(entries, graph, value))
            .collect(),
        closure: closure
            .iter()
            .map(|value| object_projection(entries, graph, value))
            .collect(),
    }
}

fn object_projection(entries: &[StoreEntry], graph: &ClosureGraph, digest: &str) -> ExplainObject {
    let object = graph.objects.get(digest);
    let owners = graph
        .records
        .values()
        .filter(|record| record.outputs.values().any(|output| output == digest))
        .map(|record| {
            entries
                .iter()
                .find(|entry| entry.id == record.id)
                .map(|entry| format!("{}@{} ({})", entry.name, entry.version, entry.reference))
                .or_else(|| {
                    super::parse_meta(&record.package_meta)
                        .map(|meta| format!("{}@{} ({})", meta.name, meta.version, meta.reference))
                })
                .unwrap_or_else(|| format!("record:{}", record.id))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    ExplainObject {
        digest: digest.to_string(),
        path: object.map(|object| object.path.clone()).unwrap_or_default(),
        external: object.is_some_and(|object| object.external),
        known: object.is_some(),
        owners,
    }
}

fn liveness_projection(
    roots: &Roots,
    entry: &StoreEntry,
    graph: Option<&ClosureGraph>,
    reports: &mut Vec<ExplainReport>,
) -> ExplainLiveness {
    let mut reasons = Vec::new();
    let lock_match = project_lock_matches(entry, reports);
    if lock_match {
        let lock = std::env::current_dir()
            .ok()
            .and_then(|cwd| super::nearest_lock_path(&cwd).ok().flatten());
        let id = lock
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "nearest-project-lock".to_string());
        reasons.push(ExplainRoot {
            kind: "project-lock".to_string(),
            id: id.clone(),
            producer: "jetpack.lock".to_string(),
            phase: "committed".to_string(),
            label: "current project lock".to_string(),
            reference: entry.reference.clone(),
            targets: vec![entry.envelope.output_hash.clone()],
            reason: format!("`{id}` names or hashes this package"),
        });
    }

    match Lifecycle::snapshot(roots) {
        Ok(snapshot) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0);
            for root in snapshot.roots.values() {
                if root.phase == super::Lifecycle::RootPhase::Tombstoned
                    || root
                        .metadata
                        .is_expired(super::Lifecycle::LifecycleTimestamp::from_unix_seconds(now))
                {
                    continue;
                }
                let protects = root.protected_targets.iter().any(|target| {
                    target == &entry.id
                        || target == &entry.envelope.output_hash
                        || graph.is_some_and(|graph| {
                            graph.closure(target).contains(&entry.envelope.output_hash)
                        })
                });
                if protects {
                    reasons.push(ExplainRoot {
                        kind: root_kind(root.identity.kind).to_string(),
                        id: root.identity.id.as_str().to_string(),
                        producer: root.identity.producer.as_str().to_string(),
                        phase: root_phase(root.phase).to_string(),
                        label: root.metadata.label.clone().unwrap_or_default(),
                        reference: root.metadata.reference.clone().unwrap_or_default(),
                        targets: root.protected_targets.iter().cloned().collect(),
                        reason: "active lifecycle root protects this object or its closure"
                            .to_string(),
                    });
                }
            }
        }
        Err(error) => reports.push(ExplainReport {
            kind: "loss".to_string(),
            message: format!("lifecycle liveness roots are unavailable: {error}"),
        }),
    }
    reasons.sort_by(|left, right| {
        (left.kind.as_str(), left.id.as_str()).cmp(&(right.kind.as_str(), right.id.as_str()))
    });
    let live = !reasons.is_empty();
    ExplainLiveness {
        live,
        roots: reasons,
        reason: if live {
            "one or more active roots protect this package".to_string()
        } else {
            "no active project-lock or lifecycle root protects this package".to_string()
        },
    }
}

fn project_lock_matches(entry: &StoreEntry, reports: &mut Vec<ExplainReport>) -> bool {
    let Ok(cwd) = std::env::current_dir() else {
        reports.push(ExplainReport {
            kind: "loss".to_string(),
            message:
                "current project directory is unavailable; project-lock liveness cannot be checked"
                    .to_string(),
        });
        return false;
    };
    let lock_path = match super::nearest_lock_path(&cwd) {
        Ok(Some(path)) => path,
        Ok(None) => return false,
        Err(error) => {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!("project lock path could not be inspected: {error}"),
            });
            return false;
        }
    };
    let Ok(raw) = fs::read_to_string(&lock_path) else {
        reports.push(ExplainReport {
            kind: "loss".to_string(),
            message: format!(
                "project lock `{}` is present but could not be read",
                lock_path.display()
            ),
        });
        return false;
    };
    let lock = match crate::Lock::parse(&raw) {
        Ok(lock) => lock,
        Err(error) => {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!(
                    "project lock `{}` could not be parsed: {error}",
                    lock_path.display()
                ),
            });
            return false;
        }
    };
    lock.packages.iter().any(|package| {
        package.name == entry.name
            && if !entry.envelope.output_hash.is_empty() {
                package
                    .envelope
                    .as_ref()
                    .is_some_and(|envelope| envelope.output_hash == entry.envelope.output_hash)
            } else {
                package.version == entry.version
            }
    }) || lock.toolchains.iter().any(|toolchain| {
        toolchain.id == entry.id
            || (!entry.envelope.output_hash.is_empty()
                && toolchain.envelope.output_hash == entry.envelope.output_hash)
    })
}

fn root_kind(kind: super::Lifecycle::RootKind) -> &'static str {
    match kind {
        super::Lifecycle::RootKind::ProjectLock => "project-lock",
        super::Lifecycle::RootKind::ProfileGeneration => "profile-generation",
        super::Lifecycle::RootKind::Toolchain => "toolchain",
        super::Lifecycle::RootKind::ExternalConsumer => "external-consumer",
        super::Lifecycle::RootKind::Manual => "manual",
    }
}

fn root_phase(phase: super::Lifecycle::RootPhase) -> &'static str {
    match phase {
        super::Lifecycle::RootPhase::Prepared => "prepared",
        super::Lifecycle::RootPhase::Committed => "committed",
        super::Lifecycle::RootPhase::Tombstoned => "tombstoned",
    }
}

fn provider_is_usable(provider: &ExplainProvider) -> bool {
    let Some(facts) = provider.provider_facts.as_ref() else {
        return false;
    };
    facts.validate().is_ok()
        && provider
            .producer_facts
            .get("provider-facts-digest")
            .is_some_and(|digest| digest == &facts.digest())
}

fn rebuild_projection(
    roots: &Roots,
    entry: &StoreEntry,
    graph: Option<&ClosureGraph>,
    provider: Option<&ExplainProvider>,
    receipt_ok: bool,
    reports: &mut Vec<ExplainReport>,
) -> ExplainRebuild {
    let mut checks = BTreeMap::new();
    let provider_usable = provider.is_some_and(provider_is_usable);
    let cache_admissions = match super::Cache::cache_admissions_for_explain(roots, entry) {
        Ok(admissions) => admissions
            .into_iter()
            .map(|admission| ExplainCacheAdmission {
                role: admission.role,
                decision: admission.decision,
                builder: admission.builder,
                provenance: admission.provenance,
                receipt_version: admission.receipt_version,
                receipt_expires_unix: admission.receipt_expires_unix,
                reason: admission.reason,
            })
            .collect(),
        Err(error) => {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!("cache admission state is unavailable: {error}"),
            });
            Vec::new()
        }
    };
    let output_exists = fs::symlink_metadata(&entry.out)
        .map(|metadata| {
            !metadata.file_type().is_symlink() && (metadata.is_file() || metadata.is_dir())
        })
        .unwrap_or(false);
    checks.insert(
        "output".to_string(),
        if output_exists { "present" } else { "missing" }.to_string(),
    );
    let output_digest = if output_exists {
        match crate::Envelope::try_output_hash_of_in_hangar(&entry.out, &roots.hangar_dir(), false)
        {
            Ok(actual) if actual == entry.envelope.output_hash => {
                checks.insert("output_digest".to_string(), "matches".to_string());
                Some(true)
            }
            Ok(actual) => {
                checks.insert("output_digest".to_string(), "mismatch".to_string());
                reports.push(ExplainReport {
                    kind: "loss".to_string(),
                    message: format!(
                        "stored output digest differs: expected {} but found {actual}",
                        entry.envelope.output_hash
                    ),
                });
                Some(false)
            }
            Err(error) => {
                checks.insert("output_digest".to_string(), "unavailable".to_string());
                reports.push(ExplainReport {
                    kind: "loss".to_string(),
                    message: format!("stored output digest could not be verified: {error}"),
                });
                None
            }
        }
    } else {
        checks.insert("output_digest".to_string(), "missing".to_string());
        None
    };
    let action_key = entry_action_key(entry);
    let action_recorded = graph.is_some_and(|graph| {
        graph.records.values().any(|record| {
            record.action_key == action_key
                && record
                    .outputs
                    .values()
                    .any(|output| output == &entry.envelope.output_hash)
        })
    });
    checks.insert(
        "closure".to_string(),
        if action_recorded {
            "recorded"
        } else {
            "missing"
        }
        .to_string(),
    );
    checks.insert(
        "producer".to_string(),
        if provider_usable {
            "decoded"
        } else {
            "missing"
        }
        .to_string(),
    );
    checks.insert(
        "receipt".to_string(),
        if receipt_ok {
            "verified"
        } else {
            "unavailable"
        }
        .to_string(),
    );
    if !output_exists {
        reports.push(ExplainReport {
            kind: "loss".to_string(),
            message: format!("stored output path `{}` is missing or invalid", entry.out),
        });
    }
    if !action_recorded {
        reports.push(ExplainReport {
            kind: "loss".to_string(),
            message: format!("stored action `{action_key}` has no matching closure record"),
        });
    }
    let attempt = match BuildDebug::latest(&roots.hangar_dir(), &entry.name) {
        Ok(attempt) => attempt.as_ref().map(attempt_projection),
        Err(error) => {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!("latest build attempt is unavailable: {error}"),
            });
            None
        }
    };
    let (decision, reason) = match (
        &attempt,
        output_exists,
        output_digest,
        action_recorded,
        provider_usable,
        receipt_ok,
    ) {
        (Some(attempt), _, _, _, _, _) if attempt.status == "failed" => (
            "rebuild-required".to_string(),
            format!("latest recorded build attempt failed at step {}", attempt.failed_step),
        ),
        (_, false, _, _, _, _) => (
            "rebuild-required".to_string(),
            "the realized output is missing; the stored action cannot be used as a live result".to_string(),
        ),
        (_, true, Some(false), _, _, _) => (
            "rebuild-required".to_string(),
            "the realized output digest does not match its stored identity".to_string(),
        ),
        (_, true, None, _, _, _) => (
            "rebuild-required".to_string(),
            "the realized output digest cannot be verified".to_string(),
        ),
        (_, true, Some(true), false, _, _) => (
            "rebuild-required".to_string(),
            "the closure does not prove the stored action/output relation".to_string(),
        ),
        (_, true, Some(true), _, false, _) => (
            "rebuild-required".to_string(),
            "producer provenance is unavailable; identity cannot be safely reused".to_string(),
        ),
        (_, true, Some(true), _, _, false) => (
            "rebuild-required".to_string(),
            "the connected Hangar receipt is unavailable or disagrees with its projection".to_string(),
        ),
        _ => (
            "realized".to_string(),
            "output, producer provenance, and action/closure identity are present; no rebuild trigger is recorded".to_string(),
        ),
    };
    ExplainRebuild {
        decision,
        reason,
        action_key,
        cache_identity: BTreeMap::from([
            (
                "source".to_string(),
                entry.cache_identity.source_fingerprint.clone(),
            ),
            (
                "recipe".to_string(),
                entry.cache_identity.recipe_fingerprint.clone(),
            ),
            (
                "policy".to_string(),
                entry.cache_identity.policy_fingerprint.clone(),
            ),
            (
                "platform".to_string(),
                entry.cache_identity.platform.clone(),
            ),
        ]),
        cache_admissions,
        checks,
        attempt,
    }
}

fn rebuild_without_entry(attempt: &BuildDebug::Attempt) -> ExplainRebuild {
    ExplainRebuild {
        decision: "rebuild-required".to_string(),
        reason: if attempt.status == "failed" {
            format!(
                "latest recorded build attempt failed at step {} and produced no StoreEntry",
                attempt.failed_step
            )
        } else {
            "build attempt produced no StoreEntry yet".to_string()
        },
        attempt: Some(attempt_projection(attempt)),
        ..Default::default()
    }
}

fn attempt_projection(attempt: &BuildDebug::Attempt) -> ExplainAttempt {
    ExplainAttempt {
        id: attempt.id.clone(),
        package: attempt.package.clone(),
        reference: attempt.reference.clone(),
        provider: attempt.provider.clone(),
        status: attempt.status.clone(),
        failed_step: attempt.failed_step,
        scratch_dir: attempt.scratch_dir.clone(),
        log_dir: attempt.log_dir.clone(),
    }
}

impl PackageExplain {
    pub fn to_json(&self) -> String {
        let conflicted = self.reports.iter().any(|report| report.kind == "conflict");
        jet_foundation::Report::render_status_json(
            if conflicted { "conflict" } else { "ok" },
            !conflicted,
            "explain",
            &format!(
                ",\"query\":{},\"lens\":{},\"entry\":{},\"provider\":{},\"graph\":{},\"liveness\":{},\"rebuild\":{},\"reports\":{}",
                JSON::quote(&self.query),
                JSON::quote(&self.lens),
                option_json(self.entry.as_ref(), ExplainEntry::to_json),
                option_json(self.provider.as_ref(), ExplainProvider::to_json),
                self.graph.to_json(),
                self.liveness.to_json(),
                self.rebuild.to_json(),
                json_array(self.reports.iter().map(ExplainReport::to_json)),
            ),
        )
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("package explain {}\n", self.query));
        out.push_str(&format!("view     {}\n", self.lens));
        if let Some(entry) = &self.entry {
            out.push_str(&format!(
                "entry    {} {} {}\nref      {}\noutput   {}\n",
                entry.name,
                empty_dash(&entry.version),
                entry.id,
                entry.reference,
                entry.output_hash
            ));
            out.push_str(&format!("receipt  {}\n", empty_dash(&entry.receipt)));
        } else {
            out.push_str("entry    - (no realized StoreEntry)\n");
            if let Some(attempt) = &self.rebuild.attempt {
                out.push_str(&format!(
                    "ref      {}\nprovider {}\nstatus   {}\nlogs     jet logs {}\n",
                    attempt.reference, attempt.provider, attempt.status, attempt.package
                ));
            }
        }
        if let Some(provider) = &self.provider {
            out.push_str(&format!(
                "provider {}\nsource   {}\nsource-digest {}\n",
                provider.provider, provider.immutable_source, provider.source_digest
            ));
            if let Some(facts) = &provider.provider_facts {
                for line in facts.explain_lines() {
                    out.push_str("fact     ");
                    out.push_str(&line);
                    out.push('\n');
                }
            }
            if let Some(facts) = &provider.locked_provider_facts {
                out.push_str(&format!("lock-facts {}\n", facts.digest()));
                for line in facts.explain_lines() {
                    out.push_str("lock-fact ");
                    out.push_str(&line);
                    out.push('\n');
                }
            }
            for profile in &provider.profile_facts {
                out.push_str(&format!(
                    "profile-facts {} generation {} output {}\n",
                    profile.profile, profile.generation, profile.output_hash
                ));
                for line in profile.provider_facts.explain_lines() {
                    out.push_str("profile-fact ");
                    out.push_str(&line);
                    out.push('\n');
                }
            }
        }
        match ExplainLens::parse(&self.lens).unwrap_or(ExplainLens::All) {
            ExplainLens::All => {
                append_graph_text(&mut out, &self.graph);
                append_liveness_text(&mut out, &self.liveness);
                append_rebuild_text(&mut out, &self.rebuild);
            }
            ExplainLens::WhyDepends => append_objects_text(
                &mut out,
                "direct dependencies",
                &self.graph.direct_dependencies,
            ),
            ExplainLens::WhatDepends => {
                append_objects_text(&mut out, "direct referrers", &self.graph.direct_referrers)
            }
            ExplainLens::Closure => append_objects_text(&mut out, "closure", &self.graph.closure),
            ExplainLens::WhyLive => append_liveness_text(&mut out, &self.liveness),
            ExplainLens::Rebuild => append_rebuild_text(&mut out, &self.rebuild),
        }
        if !self.reports.is_empty() {
            out.push_str("reports\n");
            for report in &self.reports {
                out.push_str(&format!("{}: {}\n", report.kind, report.message));
            }
        }
        out
    }
}

impl ExplainEntry {
    fn to_json(&self) -> String {
        format!(
            "{{\"id\":{},\"name\":{},\"version\":{},\"reference\":{},\"output_hash\":{},\"output_path\":{},\"bin_path\":{},\"rlib_path\":{},\"receipt\":{},\"realized_at\":{},\"last_used_at\":{}}}",
            JSON::quote(&self.id),
            JSON::quote(&self.name),
            JSON::quote(&self.version),
            JSON::quote(&self.reference),
            JSON::quote(&self.output_hash),
            JSON::quote(&self.output_path),
            JSON::quote(&self.bin_path),
            JSON::quote(&self.rlib_path),
            JSON::quote(&self.receipt),
            self.realized_at,
            self.last_used_at,
        )
    }
}

impl ExplainProvider {
    fn to_json(&self) -> String {
        let facts = self
            .producer_facts
            .iter()
            .map(|(key, value)| format!("{}:{}", JSON::quote(key), JSON::quote(value)))
            .collect::<Vec<_>>()
            .join(",");
        let profiles = self
            .profile_facts
            .iter()
            .map(ExplainProfile::to_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"provider\":{},\"immutable_source\":{},\"source_digest\":{},\"toolchain_facts\":{},\"policy_facts\":{},\"producer_facts\":{{{}}},\"provider_facts\":{},\"locked_provider_facts\":{},\"profile_facts\":[{}],\"native_document\":{}}}",
            JSON::quote(&self.provider),
            JSON::quote(&self.immutable_source),
            JSON::quote(&self.source_digest),
            JSON::quote(&self.toolchain_facts),
            JSON::quote(&self.policy_facts),
            facts,
            option_json(self.provider_facts.as_ref(), ProviderFacts::to_json),
            option_json(self.locked_provider_facts.as_ref(), ProviderFacts::to_json),
            profiles,
            JSON::quote(&self.native_document),
        )
    }
}

impl ExplainProfile {
    fn to_json(&self) -> String {
        format!(
            "{{\"profile\":{},\"generation\":{},\"output_hash\":{},\"provider_facts\":{}}}",
            JSON::quote(&self.profile),
            self.generation,
            JSON::quote(&self.output_hash),
            self.provider_facts.to_json(),
        )
    }
}

impl ExplainGraph {
    fn to_json(&self) -> String {
        format!(
            "{{\"direct_dependencies\":{},\"dependencies\":{},\"direct_referrers\":{},\"referrers\":{},\"closure\":{}}}",
            json_array(self.direct_dependencies.iter().map(ExplainObject::to_json)),
            json_array(self.dependencies.iter().map(ExplainObject::to_json)),
            json_array(self.direct_referrers.iter().map(ExplainObject::to_json)),
            json_array(self.referrers.iter().map(ExplainObject::to_json)),
            json_array(self.closure.iter().map(ExplainObject::to_json)),
        )
    }
}

impl ExplainObject {
    fn to_json(&self) -> String {
        format!(
            "{{\"digest\":{},\"path\":{},\"external\":{},\"known\":{},\"owners\":{}}}",
            JSON::quote(&self.digest),
            JSON::quote(&self.path),
            self.external,
            self.known,
            json_array(self.owners.iter().map(|owner| JSON::quote(owner))),
        )
    }
}

impl ExplainLiveness {
    fn to_json(&self) -> String {
        format!(
            "{{\"live\":{},\"reason\":{},\"roots\":{}}}",
            self.live,
            JSON::quote(&self.reason),
            json_array(self.roots.iter().map(ExplainRoot::to_json)),
        )
    }
}

impl ExplainRoot {
    fn to_json(&self) -> String {
        format!(
            "{{\"kind\":{},\"id\":{},\"producer\":{},\"phase\":{},\"label\":{},\"reference\":{},\"targets\":{},\"reason\":{}}}",
            JSON::quote(&self.kind),
            JSON::quote(&self.id),
            JSON::quote(&self.producer),
            JSON::quote(&self.phase),
            JSON::quote(&self.label),
            JSON::quote(&self.reference),
            json_array(self.targets.iter().map(|target| JSON::quote(target))),
            JSON::quote(&self.reason),
        )
    }
}

impl ExplainRebuild {
    fn to_json(&self) -> String {
        let cache = self
            .cache_identity
            .iter()
            .map(|(key, value)| format!("{}:{}", JSON::quote(key), JSON::quote(value)))
            .collect::<Vec<_>>()
            .join(",");
        let checks = self
            .checks
            .iter()
            .map(|(key, value)| format!("{}:{}", JSON::quote(key), JSON::quote(value)))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"decision\":{},\"reason\":{},\"action_key\":{},\"cache_identity\":{{{}}},\"cache_admissions\":{},\"checks\":{{{}}},\"attempt\":{}}}",
            JSON::quote(&self.decision),
            JSON::quote(&self.reason),
            JSON::quote(&self.action_key),
            cache,
            json_array(self.cache_admissions.iter().map(ExplainCacheAdmission::to_json)),
            checks,
            option_json(self.attempt.as_ref(), ExplainAttempt::to_json),
        )
    }
}

impl ExplainReport {
    fn to_json(&self) -> String {
        let conflict = self.kind == "conflict";
        let mut report = ReportEnvelope::new(
            "tool",
            if conflict { "error" } else { "warning" },
            "E1340",
            self.message.as_str(),
            if conflict {
                "two persisted Hangar facts disagree, so the explanation cannot claim one identity"
            } else {
                "a persisted Hangar fact needed by this explanation is unavailable or lossy"
            },
            if conflict {
                "repair the conflicting Hangar or provider record, then run `jet explain` again"
            } else {
                "run `jetpack hangar verify` and repair or rebuild the affected package before relying on this fact"
            },
        )
        .json();
        report.pop();
        report.push_str(",\"kind\":");
        report.push_str(&JSON::quote(&self.kind));
        report.push('}');
        report
    }
}

impl ExplainAttempt {
    fn to_json(&self) -> String {
        format!(
            "{{\"id\":{},\"package\":{},\"reference\":{},\"provider\":{},\"status\":{},\"failed_step\":{},\"scratch_dir\":{},\"log_dir\":{}}}",
            JSON::quote(&self.id),
            JSON::quote(&self.package),
            JSON::quote(&self.reference),
            JSON::quote(&self.provider),
            JSON::quote(&self.status),
            self.failed_step,
            JSON::quote(&self.scratch_dir),
            JSON::quote(&self.log_dir),
        )
    }
}

fn append_graph_text(out: &mut String, graph: &ExplainGraph) {
    append_objects_text(out, "direct dependencies", &graph.direct_dependencies);
    append_objects_text(out, "transitive dependencies", &graph.dependencies);
    append_objects_text(out, "direct referrers", &graph.direct_referrers);
    append_objects_text(out, "transitive referrers", &graph.referrers);
    append_objects_text(out, "closure", &graph.closure);
}

fn append_objects_text(out: &mut String, title: &str, objects: &[ExplainObject]) {
    out.push_str(title);
    out.push('\n');
    if objects.is_empty() {
        out.push_str("  -\n");
        return;
    }
    for object in objects {
        let owners = if object.owners.is_empty() {
            "-".to_string()
        } else {
            object.owners.join(", ")
        };
        out.push_str(&format!(
            "  {}  {}  owners: {}{}\n",
            object.digest,
            if object.known {
                empty_dash(&object.path)
            } else {
                "<unknown>"
            },
            owners,
            if object.external { "  external" } else { "" }
        ));
    }
}

fn append_liveness_text(out: &mut String, liveness: &ExplainLiveness) {
    out.push_str(&format!(
        "liveness {}  {}\n",
        if liveness.live { "live" } else { "unrooted" },
        liveness.reason
    ));
    for root in &liveness.roots {
        out.push_str(&format!(
            "  root {} {} {}\n",
            root.kind, root.id, root.reason
        ));
    }
}

fn append_rebuild_text(out: &mut String, rebuild: &ExplainRebuild) {
    out.push_str(&format!(
        "rebuild {}  {}\naction   {}\n",
        rebuild.decision, rebuild.reason, rebuild.action_key
    ));
    for (key, value) in &rebuild.cache_identity {
        out.push_str(&format!("cache {} {}\n", key, value));
    }
    for admission in &rebuild.cache_admissions {
        out.push_str(&format!(
            "cache-trust {} {} builder={} provenance={} receipt-version={} receipt-expires={} {}\n",
            admission.role,
            admission.decision,
            empty_dash(&admission.builder),
            empty_dash(&admission.provenance),
            admission
                .receipt_version
                .map(|version| version.to_string())
                .unwrap_or_else(|| "-".to_string()),
            admission
                .receipt_expires_unix
                .map(|expires| expires.to_string())
                .unwrap_or_else(|| "-".to_string()),
            admission.reason,
        ));
    }
    for (key, value) in &rebuild.checks {
        out.push_str(&format!("check {} {}\n", key, value));
    }
    if let Some(attempt) = &rebuild.attempt {
        out.push_str(&format!(
            "attempt {} {} {}\n",
            attempt.id, attempt.status, attempt.reference
        ));
    }
}

impl ExplainCacheAdmission {
    fn to_json(&self) -> String {
        format!(
            "{{\"role\":{},\"decision\":{},\"builder\":{},\"provenance\":{},\"receipt_version\":{},\"receipt_expires_unix\":{},\"reason\":{}}}",
            JSON::quote(&self.role),
            JSON::quote(&self.decision),
            JSON::quote(&self.builder),
            JSON::quote(&self.provenance),
            self.receipt_version
                .map(|version| version.to_string())
                .unwrap_or_else(|| "null".to_string()),
            self.receipt_expires_unix
                .map(|expires| expires.to_string())
                .unwrap_or_else(|| "null".to_string()),
            JSON::quote(&self.reason),
        )
    }
}

fn json_array<I>(values: I) -> String
where
    I: IntoIterator<Item = String>,
{
    format!("[{}]", values.into_iter().collect::<Vec<_>>().join(","))
}

fn option_json<T>(value: Option<&T>, render: impl Fn(&T) -> String) -> String {
    value.map(render).unwrap_or_else(|| "null".to_string())
}

fn empty_dash(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
    }
}

#[cfg(test)]
mod why_projection_tests {
    use super::*;

    #[test]
    fn requesting_source_matching_ignores_comments_and_prefix_collisions() {
        let names = vec!["ripgrep".to_string()];
        let qualified = vec!["ripgrep@default".to_string()];
        assert!(source_line_matches("        ripgrep,", &names, &qualified));
        assert!(source_line_matches(
            "        ripgrep@default,",
            &names,
            &qualified
        ));
        assert!(!source_line_matches("// ripgrep,", &names, &qualified));
        assert!(!source_line_matches("        ripgrep2,", &names, &qualified));
    }

    #[test]
    fn lock_source_matching_stays_inside_package_records() {
        let raw = "version = 1\n[root]\ndependencies = [\"ripgrep\"]\n\n[[package]]\nname = \"ripgrep2\"\n\n[[package]]\nname = \"ripgrep\"\n";
        let names = vec!["ripgrep".to_string()];
        let found = lock_line(raw, &names, &[]).expect("package lock line");
        assert_eq!(found.1, "name = \"ripgrep\"");
        assert_eq!(found.0, 9);
    }

    #[test]
    fn trust_projection_covers_admission_grades() {
        let reproduced = BTreeMap::from([(
            "cache.reproducibility".to_string(),
            "independent-agreeing-v1:sha256-proof".to_string(),
        )]);
        assert_eq!(
            trust_projection("native", Some(&reproduced)).grade,
            "reproduced"
        );

        let native = BTreeMap::from([
            (
                "source.kind".to_string(),
                "local-unofficial-catalog".to_string(),
            ),
            (
                "nix.index.tier".to_string(),
                "local-unofficial".to_string(),
            ),
        ]);
        let trust = trust_projection("jetpackage", Some(&native));
        assert_eq!(trust.grade, "unverified-mapping");
        assert!(trust.reason.contains("native recipe mapping"));

        let signed = BTreeMap::from([
            (
                "nix.index.tier".to_string(),
                "official-signed".to_string(),
            ),
            (
                "nix.index.signature-chain".to_string(),
                "present".to_string(),
            ),
        ]);
        assert_eq!(trust_projection("nix", Some(&signed)).grade, "signed");
    }
}
