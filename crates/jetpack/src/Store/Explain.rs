//! Read-only package explanation assembled from the Hangar fact stores.
//!
//! The CLI is deliberately a renderer. Dependency edges come from the
//! closure graph, liveness comes from lock/lifecycle roots, and rebuild facts
//! come from the stored action identity and build attempt record.

use super::{entry_action_key, ClosureGraph, Lifecycle, ProducerRecord, Roots, StoreEntry};
use crate::{BuildDebug, ProviderFactValue, ProviderFacts, SemanticLock, JSON};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

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
    pub native_document: String,
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
    pub checks: BTreeMap<String, String>,
    pub attempt: Option<ExplainAttempt>,
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
    let entries = super::list_checked(roots)?;
    let entry = find_entry(&entries, query)?;
    if entry.is_none() {
        let package = package_name(query);
        let attempt = match BuildDebug::latest(&roots.hangar_dir(), package) {
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
            kind: if attempt.status == "failed" {
                "conflict".to_string()
            } else {
                "loss".to_string()
            },
            message: rebuild.reason.clone(),
        });
        return Ok(Some(PackageExplain {
            schema: "jet-package-explain-v1".to_string(),
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
    let provider = provider_projection(&entry, &mut reports);
    let provider = provider.map(|mut provider| {
        provider.locked_provider_facts = locked_provider_facts(&entry, &mut reports);
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
        &mut reports,
    );

    Ok(Some(PackageExplain {
        schema: "jet-package-explain-v1".to_string(),
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

fn package_name(query: &str) -> &str {
    query
        .split_once(':')
        .map(|(_, value)| value)
        .or_else(|| query.split_once('.').map(|(_, value)| value))
        .unwrap_or(query)
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
    match super::closure_graph(roots) {
        Ok(graph) => Some(graph),
        Err(error) => {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!("closure proof is unavailable: {error}"),
            });
            match super::closure_graph_structure(roots) {
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
    let mut facts = ProviderFacts::for_reference(&producer.provider, &entry.reference);
    facts.set_resolved_source(&producer.immutable_source);
    facts.set_native_document("jet-producer-record-v1", &entry.producer_record);
    for (key, value) in &producer.facts {
        facts.add_fact(
            key,
            ProviderFactValue::Text(value.clone()),
            "jet-producer-record-v1",
        );
    }
    facts.add_fact(
        "provider.source_digest",
        ProviderFactValue::Text(producer.source_digest.clone()),
        "jet-producer-record-v1",
    );
    facts.add_fact(
        "provider.toolchain_facts",
        ProviderFactValue::Text(producer.toolchain_facts.clone()),
        "jet-producer-record-v1",
    );
    facts.add_fact(
        "provider.policy_facts",
        ProviderFactValue::Text(producer.policy_facts.clone()),
        "jet-producer-record-v1",
    );
    report_provider_facts(&facts, reports);
    Some(ExplainProvider {
        provider: producer.provider,
        immutable_source: producer.immutable_source,
        source_digest: producer.source_digest,
        toolchain_facts: producer.toolchain_facts,
        policy_facts: producer.policy_facts,
        producer_facts: producer.facts,
        provider_facts: Some(facts),
        locked_provider_facts: None,
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

fn locked_provider_facts(
    entry: &StoreEntry,
    reports: &mut Vec<ExplainReport>,
) -> Option<ProviderFacts> {
    let cwd = std::env::current_dir().ok()?;
    let lock_path = super::nearest_lock_path(&cwd)?;
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
    for record in lock.records {
        let Some(raw) = record.future_fields.get("provider-facts") else {
            continue;
        };
        match ProviderFacts::from_json(raw) {
            Ok(facts) => {
                let canonical_entry =
                    ProviderFacts::for_reference("", &entry.reference).qualified_reference();
                if let Err(error) = facts.validate() {
                    reports.push(ExplainReport {
                        kind: "loss".to_string(),
                        message: format!("locked provider facts are invalid: {error}"),
                    });
                } else if facts.reference == entry.reference
                    || facts.qualified_reference() == canonical_entry
                    || record.identity.exact == entry.reference
                    || record.identity.exact == facts.qualified_reference()
                {
                    matches.push(facts);
                }
            }
            Err(error) => reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!("locked provider facts could not be decoded: {error}"),
            }),
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
            .and_then(|cwd| super::nearest_lock_path(&cwd));
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
    let Some(lock_path) = super::nearest_lock_path(&cwd) else {
        return false;
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

fn rebuild_projection(
    roots: &Roots,
    entry: &StoreEntry,
    graph: Option<&ClosureGraph>,
    provider: Option<&ExplainProvider>,
    reports: &mut Vec<ExplainReport>,
) -> ExplainRebuild {
    let mut checks = BTreeMap::new();
    let output_exists = fs::symlink_metadata(&entry.out)
        .map(|metadata| {
            !metadata.file_type().is_symlink() && (metadata.is_file() || metadata.is_dir())
        })
        .unwrap_or(false);
    checks.insert(
        "output".to_string(),
        if output_exists { "present" } else { "missing" }.to_string(),
    );
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
        if provider.is_some() {
            "decoded"
        } else {
            "missing"
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
        Ok(attempt) => attempt.map(attempt_projection),
        Err(error) => {
            reports.push(ExplainReport {
                kind: "loss".to_string(),
                message: format!("latest build attempt is unavailable: {error}"),
            });
            None
        }
    };
    let (decision, reason) = match (&attempt, output_exists, action_recorded, provider.is_some()) {
        (Some(attempt), _, _, _) if attempt.status == "failed" => (
            "rebuild-required".to_string(),
            format!("latest recorded build attempt failed at step {}", attempt.failed_step),
        ),
        (_, false, _, _) => (
            "rebuild-required".to_string(),
            "the realized output is missing; the stored action cannot be used as a live result".to_string(),
        ),
        (_, _, false, _) => (
            "rebuild-required".to_string(),
            "the closure does not prove the stored action/output relation".to_string(),
        ),
        (_, _, _, false) => (
            "rebuild-required".to_string(),
            "producer provenance is unavailable; identity cannot be safely reused".to_string(),
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
        format!(
            "{{\"schema\":{},\"query\":{},\"lens\":{},\"entry\":{},\"provider\":{},\"graph\":{},\"liveness\":{},\"rebuild\":{},\"reports\":{}}}",
            JSON::quote(&self.schema),
            JSON::quote(&self.query),
            JSON::quote(&self.lens),
            option_json(self.entry.as_ref(), ExplainEntry::to_json),
            option_json(self.provider.as_ref(), ExplainProvider::to_json),
            self.graph.to_json(),
            self.liveness.to_json(),
            self.rebuild.to_json(),
            json_array(self.reports.iter().map(ExplainReport::to_json)),
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
        format!(
            "{{\"provider\":{},\"immutable_source\":{},\"source_digest\":{},\"toolchain_facts\":{},\"policy_facts\":{},\"producer_facts\":{{{}}},\"provider_facts\":{},\"locked_provider_facts\":{},\"native_document\":{}}}",
            JSON::quote(&self.provider),
            JSON::quote(&self.immutable_source),
            JSON::quote(&self.source_digest),
            JSON::quote(&self.toolchain_facts),
            JSON::quote(&self.policy_facts),
            facts,
            option_json(self.provider_facts.as_ref(), ProviderFacts::to_json),
            option_json(self.locked_provider_facts.as_ref(), ProviderFacts::to_json),
            JSON::quote(&self.native_document),
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
            "{{\"decision\":{},\"reason\":{},\"action_key\":{},\"cache_identity\":{{{}}},\"checks\":{{{}}},\"attempt\":{}}}",
            JSON::quote(&self.decision),
            JSON::quote(&self.reason),
            JSON::quote(&self.action_key),
            cache,
            checks,
            option_json(self.attempt.as_ref(), ExplainAttempt::to_json),
        )
    }
}

impl ExplainReport {
    fn to_json(&self) -> String {
        format!(
            "{{\"kind\":{},\"message\":{}}}",
            JSON::quote(&self.kind),
            JSON::quote(&self.message)
        )
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
