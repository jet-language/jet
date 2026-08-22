//! Read-only package explanation assembled from the Hangar fact stores.
//!
//! The CLI is deliberately a renderer. Dependency edges come from the
//! closure graph, liveness comes from lock/lifecycle roots, and rebuild facts
//! come from the stored action identity and build attempt record.

use super::{entry_action_key, ClosureGraph, Lifecycle, ProducerRecord, Roots, StoreEntry};
use crate::{BuildDebug, ProviderFacts, SemanticLock, JSON};
use jet_foundation::Report::ReportEnvelope;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

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
                message: format!("Hangar receipt `{}` could not be read: {error}", entry.receipt),
            }),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => reports.push(ExplainReport {
            kind: "loss".to_string(),
            message: format!("Hangar receipt `{}` is missing", entry.receipt),
        }),
        Err(error) => reports.push(ExplainReport {
            kind: "loss".to_string(),
            message: format!("Hangar receipt `{}` could not be inspected: {error}", entry.receipt),
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
        if receipt_ok { "verified" } else { "unavailable" }.to_string(),
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
                "run `jet hangar verify` and repair or rebuild the affected package before relying on this fact"
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
