//! Editable migration-import IR (D-WD5).
//!
//! Command spelling and TODO diagnostic codes are gated. These helpers produce
//! canonical editable files and data-only TODO facts for callers/tests.

use super::ProviderGraph::{normalize_provider_document, ProviderFamily};
use super::JSON::{self, JSONValue};
use jet_pkg_model::ProviderFacts::{ProviderFactValue, ProviderFacts};
use std::collections::BTreeMap;

fn source_path_for_finding(source: &str, fallback: &str) -> String {
    if !fallback.trim().is_empty()
        && (matches!(
            source,
            "" | "provider.native_document" | "reference.provider" | "reference.selector"
        ) || source.starts_with("package.json."))
    {
        return fallback.to_string();
    }
    if source.trim().is_empty() {
        return if fallback.trim().is_empty() {
            "provider facts".to_string()
        } else {
            fallback.to_string()
        };
    }
    source.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPlan {
    pub source_kind: String,
    pub packages: Vec<ImportedPackage>,
    pub deps: Vec<ImportedDep>,
    pub generated_files: Vec<GeneratedFile>,
    pub adapters: Vec<AdapterDraft>,
    pub ffi_stubs: Vec<FFIStub>,
    pub todos: Vec<ImportTodo>,
    pub migration: Vec<MigrationStatus>,
    /// One shared provider-fact record per imported provider reference. Losses
    /// remain on the record so generated files cannot hide an unresolved fact.
    pub provider_facts: BTreeMap<String, ProviderFacts>,
}

impl ImportPlan {
    pub fn empty(kind: impl Into<String>) -> ImportPlan {
        ImportPlan {
            source_kind: kind.into(),
            packages: Vec::new(),
            deps: Vec::new(),
            generated_files: Vec::new(),
            adapters: Vec::new(),
            ffi_stubs: Vec::new(),
            todos: Vec::new(),
            migration: Vec::new(),
            provider_facts: BTreeMap::new(),
        }
    }

    fn retain_provider_facts(&mut self, facts: ProviderFacts) {
        self.retain_provider_facts_with_source(facts, "");
    }

    fn retain_provider_facts_with_source(&mut self, facts: ProviderFacts, source_path: &str) {
        for loss in &facts.losses {
            self.record_provider_finding(
                source_path_for_finding(&loss.source, source_path),
                format!(
                    "provider fact `{}` is lossy: {}; migration remains unresolved",
                    loss.key, loss.reason
                ),
            );
        }
        for conflict in &facts.conflicts {
            self.record_provider_finding(
                source_path_for_finding(&conflict.source, source_path),
                format!(
                    "provider fact `{}` conflicts: {} vs {}; migration is unresolved",
                    conflict.key, conflict.left, conflict.right
                ),
            );
        }
        let reference = facts.reference.clone();
        let Some(existing) = self.provider_facts.get_mut(&reference) else {
            self.provider_facts.insert(reference, facts);
            return;
        };
        if existing.provider != facts.provider {
            let left = existing.provider.clone();
            let right = facts.provider.clone();
            existing.add_conflict("provider", &left, &right, "provider.import");
        }
        if existing.target != facts.target {
            let left = existing.target.clone();
            let right = facts.target.clone();
            existing.add_conflict("target", &left, &right, "provider.import");
        }
        if !facts.resolved_source.is_empty() {
            existing.set_resolved_source(&facts.resolved_source);
        }
        if !facts.native_format.is_empty() {
            existing.set_native_document(&facts.native_format, &facts.native_document);
        }
        for (key, value) in &facts.facts {
            let source = facts
                .provenance
                .get(key)
                .map(String::as_str)
                .unwrap_or("provider.import");
            existing.add_fact(key, value.clone(), source);
        }
        for loss in facts.losses {
            if !existing.losses.contains(&loss) {
                existing.losses.push(loss);
            }
        }
        for conflict in facts.conflicts {
            if !existing.conflicts.contains(&conflict) {
                existing.conflicts.push(conflict);
            }
        }
        let merged_losses = self
            .provider_facts
            .get(&reference)
            .map(|merged| merged.losses.clone())
            .unwrap_or_default();
        let merged_conflicts = self
            .provider_facts
            .get(&reference)
            .map(|merged| merged.conflicts.clone())
            .unwrap_or_default();
        for loss in merged_losses {
            self.record_provider_finding(
                source_path_for_finding(&loss.source, source_path),
                format!(
                    "provider fact `{}` is lossy: {}; migration remains unresolved",
                    loss.key, loss.reason
                ),
            );
        }
        for conflict in merged_conflicts {
            self.record_provider_finding(
                source_path_for_finding(&conflict.source, source_path),
                format!(
                    "provider fact `{}` conflicts: {} vs {}; migration is unresolved",
                    conflict.key, conflict.left, conflict.right
                ),
            );
        }
    }

    fn record_provider_finding(&mut self, source_path: String, message: String) {
        let todo = ImportTodo {
            source_path,
            message,
            suggested_surface: None,
        };
        if !self.todos.contains(&todo) {
            self.todos.push(todo);
        }
    }

    pub fn emit_pkg_jet(&self) -> String {
        let name = self
            .packages
            .first()
            .map(|p| p.name.as_str())
            .unwrap_or("imported-app");
        let version = self
            .packages
            .first()
            .map(|p| p.version.as_str())
            .unwrap_or("0.1.0");
        let mut out = format!(
            "payload: {{\n    name: \"{name}\",\n    version: \"{version}\",\n    jet: \">=0.1.0\",\n    description: \"Imported from {}\",\n    license: \"\",\n    repository: \"\",\n}}\n",
            self.source_kind
        );
        out.push_str("\ndeps: {\n");
        let unresolved_root = self.provider_facts.iter().any(|(reference, facts)| {
            !facts.is_lossless()
                && !self
                    .deps
                    .iter()
                    .any(|dependency| dependency.provider_ref == *reference)
        });
        if unresolved_root {
            out.push_str("}\n");
            return out;
        }
        let mut deps = self.deps.clone();
        deps.sort_by(|a, b| a.name.cmp(&b.name));
        for dep in deps {
            let Some(facts) = self.provider_facts.get(&dep.provider_ref) else {
                continue;
            };
            if !facts.is_lossless()
                || matches!(
                    facts.facts.get("package.dependency_kind"),
                    Some(ProviderFactValue::Text(kind)) if kind != "runtime"
                )
            {
                // A mutable foreign selector belongs in the source-linked
                // migration finding, never in generated Jet source.  The
                // canonical manifest has no dev/optional/peer dependency
                // section, so those roles also stay as provider facts plus a
                // migration finding instead of becoming runtime dependencies.
                continue;
            }
            let provider_ref = facts.qualified_reference();
            out.push_str(&format!("    {}: {},\n", dep.name, provider_ref));
        }
        out.push_str("}\n");
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedPackage {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedDep {
    pub name: String,
    pub provider_ref: String,
    pub locked_version: String,
    pub dev: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub path: String,
    pub contents: String,
    pub owned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterDraft {
    pub name: String,
    pub source: String,
    pub recipe: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FFIStub {
    pub path: String,
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportTodo {
    pub source_path: String,
    pub message: String,
    pub suggested_surface: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationStatus {
    ForeignDependencyRetained { name: String },
    AdapterWrapped { name: String },
    FFIStubGenerated { name: String },
    NativeReplacementCandidate { name: String },
    CompatibilityProved { name: String },
    NativeReplacementActive { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportConflict {
    UserEditedGeneratedLine { path: String },
}

pub fn todo_diagnostics_need_ballot() -> bool {
    true
}

pub fn import_nix_facts(source_path: &str, facts_json: &str) -> ImportPlan {
    let mut plan = ImportPlan::empty("nix");
    let parsed = JSON::parse(facts_json).ok();
    let obj = parsed.as_ref().and_then(|j| j.as_object().ok());
    let report = normalize_provider_document(ProviderFamily::Nix, facts_json);
    let report_name = report.facts.name.clone();
    let name = if report_name.is_empty() {
        obj.and_then(|object| nix_import_string(object, "name"))
            .unwrap_or_else(|| "nix-import".to_string())
    } else {
        report_name
    };
    let version = if report.facts.version.is_empty() || report.facts.version == "set" {
        "0.1.0".to_string()
    } else {
        report.facts.version.clone()
    };
    let root_reference = if !report.facts.name.is_empty()
        && !report.facts.version.is_empty()
        && report.facts.version != "set"
    {
        report.shared_facts().qualified_reference()
    } else {
        format!("{name}@nix")
    };
    plan.packages.push(ImportedPackage {
        name,
        version,
    });
    retain_normalized_root(
        &mut plan,
        ProviderFamily::Nix,
        facts_json,
        &root_reference,
        source_path,
    );
    if let Some(JSONValue::Array(pkgs)) = obj.and_then(|m| m.get("packages")) {
        for (index, pkg) in pkgs.iter().enumerate() {
            let Some((name, provider_ref, locked, immutable_source, exact)) =
                nix_import_package(pkg)
            else {
                plan.record_provider_finding(
                    source_path.to_string(),
                    format!(
                        "Nix provider package entry {index} has no usable package identity; migration is unresolved"
                    ),
                );
                continue;
            };
            plan.deps.push(ImportedDep {
                name: name.clone(),
                provider_ref: provider_ref.clone(),
                locked_version: locked,
                dev: false,
            });
            let mut facts = ProviderFacts::for_reference("nix", &provider_ref);
            if let JSONValue::Object(_) = pkg {
                let package_report = normalize_provider_document(
                    ProviderFamily::Nix,
                    &nix_json_value(pkg),
                );
                merge_provider_projection(
                    &mut facts,
                    &package_report.shared_facts_for(&provider_ref),
                    source_path,
                );
            }
            facts.set_native_document("flake-facts.json", facts_json);
            facts.add_fact(
                "package.name",
                ProviderFactValue::Text(name),
                source_path,
            );
            if let Some(source) = immutable_source {
                if facts.resolved_source.is_empty() {
                    facts.set_resolved_source(&source);
                }
            } else {
                facts.add_loss(
                    "provider.source",
                    "Nix package entry has no immutable source identity",
                    source_path,
                );
            }
            if !exact {
                let reason = "imported Nix package has no exact version, revision, or digest; retain the flake lock before realization";
                if !facts.losses.iter().any(|loss| loss.reason == reason) {
                    facts.add_loss("provider.selector", reason, source_path);
                }
            }
            if let JSONValue::Object(package) = pkg {
                for key in [
                    "pname",
                    "version",
                    "revision",
                    "rev",
                    "narHash",
                    "hash",
                    "drvPath",
                ] {
                    if let Some(value) = nix_import_string(package, key) {
                        facts.add_fact(
                            &format!("provider.nix.import.{key}"),
                            ProviderFactValue::Text(value),
                            source_path,
                        );
                    }
                }
            }
            plan.retain_provider_facts_with_source(facts, source_path);
        }
    } else if obj.is_some_and(|object| object.contains_key("packages")) {
        plan.record_provider_finding(
            source_path.to_string(),
            "Nix `packages` must be an array of package names or exact records".to_string(),
        );
    }
    if obj.and_then(|m| m.get("shellHook")).is_some() {
        plan.todos.push(ImportTodo {
            source_path: source_path.to_string(),
            message: "`shellHook` needs an explicit Jetpack build/env action".to_string(),
            suggested_surface: Some("role module env.dev plus declared build action".to_string()),
        });
    }
    plan.generated_files.push(GeneratedFile {
        path: "env.jet".to_string(),
        contents: "module env.dev\n".to_string(),
        owned: true,
    });
    plan
}

fn nix_import_string(
    object: &std::collections::BTreeMap<String, JSONValue>,
    key: &str,
) -> Option<String> {
    object
        .get(key)
        .and_then(|value| value.as_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn merge_provider_projection(target: &mut ProviderFacts, source: &ProviderFacts, source_path: &str) {
    if target.resolved_source.is_empty() && !source.resolved_source.is_empty() {
        target.set_resolved_source(&source.resolved_source);
    }
    for (key, value) in &source.facts {
        let provenance = source
            .provenance
            .get(key)
            .map(String::as_str)
            .unwrap_or(source_path);
        target.add_fact(key, value.clone(), provenance);
    }
    for loss in &source.losses {
        if !target.losses.contains(loss) {
            target.losses.push(loss.clone());
        }
    }
    for conflict in &source.conflicts {
        if !target.conflicts.contains(conflict) {
            target.conflicts.push(conflict.clone());
        }
    }
}

fn nix_json_value(value: &JSONValue) -> String {
    match value {
        JSONValue::Null => "null".to_string(),
        JSONValue::Bool(value) => value.to_string(),
        JSONValue::Number(value) => value.to_string(),
        JSONValue::Flt(value) => value.to_string(),
        JSONValue::String(value) => JSON::quote(value),
        JSONValue::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(nix_json_value)
                .collect::<Vec<_>>()
                .join(",")
        ),
        JSONValue::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!("{}:{}", JSON::quote(key), nix_json_value(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn nix_import_package(
    value: &JSONValue,
) -> Option<(String, String, String, Option<String>, bool)> {
    let (name, source, version, revision, digest, reference) = match value {
        JSONValue::String(raw) => {
            let raw = raw.trim();
            if raw.is_empty() {
                return None;
            }
            let name = raw
                .split_once('#')
                .map(|(name, _)| name)
                .or_else(|| raw.split_once('@').map(|(name, _)| name))
                .unwrap_or(raw)
                .to_string();
            let reference = if raw.contains('@') || raw.contains('#') {
                raw.to_string()
            } else {
                format!("{raw}@nixpkgs")
            };
            (name, None, None, None, None, Some(reference))
        }
        JSONValue::Object(object) => (
            nix_import_string(object, "name")
                .or_else(|| nix_import_string(object, "pname"))
                .or_else(|| nix_import_string(object, "package"))?,
            nix_import_string(object, "source")
                .or_else(|| nix_import_string(object, "provider")),
            nix_import_string(object, "version"),
            nix_import_string(object, "revision").or_else(|| nix_import_string(object, "rev")),
            nix_import_string(object, "digest")
                .or_else(|| nix_import_string(object, "narHash"))
                .or_else(|| nix_import_string(object, "hash"))
                .or_else(|| nix_import_string(object, "outputHash")),
            nix_import_string(object, "reference")
                .or_else(|| nix_import_string(object, "provider_ref")),
        ),
        _ => return None,
    };
    let name = name.trim().to_string();
    let provider_ref = if let Some(reference) = reference {
        reference
    } else {
        let authority = source.clone().unwrap_or_else(|| "nixpkgs".to_string());
        let selector = version
            .as_ref()
            .map(|value| format!("#version={value}"))
            .or_else(|| revision.as_ref().map(|value| format!("#revision={value}")))
            .or_else(|| digest.as_ref().map(|value| format!("#digest={value}")))
            .unwrap_or_default();
        format!("{name}{selector}@{authority}")
    };
    let selector_facts = ProviderFacts::for_reference("nix", &provider_ref);
    let locked = if !selector_facts.selector.version.is_empty() {
        selector_facts.selector.version.clone()
    } else if !selector_facts.selector.revision.is_empty() {
        selector_facts.selector.revision.clone()
    } else {
        selector_facts.selector.digest.clone()
    };
    let exact = selector_facts.selector.is_exact();
    let immutable_source = match value {
        JSONValue::Object(object) => nix_import_string(object, "drvPath")
            .or_else(|| nix_import_string(object, "immutableSource"))
            .or_else(|| nix_import_string(object, "sourceIdentity"))
            .or_else(|| nix_import_string(object, "sourcePath")),
        _ => None,
    };
    Some((name, provider_ref, locked, immutable_source, exact))
}

pub fn import_cargo(cargo_toml: &str, cargo_lock: &str) -> ImportPlan {
    let mut plan = ImportPlan::empty("cargo");
    let source_name = toml_string(cargo_toml, "name");
    let source_version = toml_string(cargo_toml, "version");
    let name = source_name
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "cargo-import".to_string());
    let version = source_version
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "0.1.0".to_string());
    let root_reference = match (source_name, source_version) {
        (Some(name), Some(version)) if !name.is_empty() && !version.is_empty() => {
            format!("{name}#version={version}@cargo")
        }
        _ => format!("{name}@cargo"),
    };
    plan.packages.push(ImportedPackage {
        name: name.clone(),
        version: version.clone(),
    });
    let root_facts = retain_normalized_root(
        &mut plan,
        ProviderFamily::Cargo,
        &format!("Cargo.toml:\n{cargo_toml}\nCargo.lock:\n{cargo_lock}"),
        &root_reference,
        "Cargo.toml",
    );
    let native_document = format!("Cargo.toml:\n{cargo_toml}\nCargo.lock:\n{cargo_lock}");
    for (section, dev, kind, platform) in cargo_dependency_sections(cargo_toml) {
        for dep in dependency_keys(cargo_toml, &section) {
            let versions = lock_versions(cargo_lock, &dep);
            let locked = (versions.len() == 1)
                .then(|| versions.first().cloned().unwrap_or_default())
                .unwrap_or_default();
            let platform_selector = platform
                .as_deref()
                .map(|platform| format!("&platform={platform}"))
                .unwrap_or_default();
            let provider_ref = if locked.is_empty() {
                platform
                    .as_deref()
                    .map(|platform| format!("{dep}#platform={platform}@cargo"))
                    .unwrap_or_else(|| format!("{dep}@cargo"))
            } else {
                format!("{dep}#version={locked}{platform_selector}@cargo")
            };
            plan.deps.push(ImportedDep {
                name: dep.clone(),
                provider_ref: provider_ref.clone(),
                locked_version: locked.clone(),
                dev,
            });
            if kind != "runtime" {
                plan.record_provider_finding(
                    "Cargo.toml".to_string(),
                    format!(
                        "Cargo `{kind}` dependency `{dep}` has no canonical Jet runtime dependency output; migration remains unresolved"
                    ),
                );
            }
            let mut facts = ProviderFacts::for_reference("cargo", &provider_ref);
            facts.set_native_document("Cargo.toml+Cargo.lock", &native_document);
            copy_provider_projection(&mut facts, &root_facts, "provider.cargo.");
            facts.add_fact(
                "package.name",
                ProviderFactValue::Text(dep.clone()),
                &format!("Cargo.toml.{section}"),
            );
            facts.add_fact(
                "package.dependency_kind",
                ProviderFactValue::Text(kind.to_string()),
                &format!("Cargo.toml.{section}"),
            );
            if let Some(platform) = &platform {
                facts.add_fact(
                    "package.platform",
                    ProviderFactValue::Text(platform.clone()),
                    &format!("Cargo.toml.{section}"),
                );
            }
            if versions.len() > 1 {
                facts.add_conflict(
                    "provider.selector.version",
                    &versions[0],
                    &versions[1..].join(","),
                    "Cargo.lock",
                );
            } else if !locked.is_empty() {
                facts.set_resolved_source(&format!("cargo:{dep}@{locked}"));
                facts.add_fact(
                    "package.version",
                    ProviderFactValue::Text(locked.clone()),
                    "Cargo.lock.version",
                );
                if let Some(source) = lock_field(cargo_lock, &dep, "source") {
                    facts.add_fact(
                        "provider.source",
                        ProviderFactValue::Text(source),
                        "Cargo.lock.source",
                    );
                }
                if let Some(checksum) = lock_field(cargo_lock, &dep, "checksum") {
                    facts.add_fact(
                        "package.integrity",
                        ProviderFactValue::Text(checksum),
                        "Cargo.lock.checksum",
                    );
                }
            } else {
                facts.add_loss(
                    "provider.selector",
                    "Cargo.lock has no exact version for this dependency",
                    "Cargo.lock",
                );
            }
            plan.retain_provider_facts_with_source(facts, "Cargo.toml");
            plan.ffi_stubs.push(FFIStub {
                path: format!("ffi/{dep}.jet"),
                symbol: dep,
            });
        }
    }
    if cargo_toml.contains("build =") {
        plan.todos.push(ImportTodo {
            source_path: "Cargo.toml".to_string(),
            message: "Cargo build script becomes a Tier-2 legacy build action".to_string(),
            suggested_surface: Some(
                "declared build wrapper with inputs, outputs, and caps".to_string(),
            ),
        });
    }
    plan
}

pub fn import_npm(package_json: &str) -> ImportPlan {
    let mut plan = ImportPlan::empty("npm");
    let parsed = JSON::parse(package_json).ok();
    let obj = parsed.as_ref().and_then(|j| j.as_object().ok());
    let source_name = obj
        .and_then(|m| m.get("name"))
        .and_then(|v| v.as_str().ok())
        .unwrap_or_default()
        .to_string();
    let source_version = obj
        .and_then(|m| m.get("version"))
        .and_then(|v| v.as_str().ok())
        .unwrap_or_default()
        .to_string();
    let name = if source_name.is_empty() {
        "npm-import".to_string()
    } else {
        source_name.clone()
    };
    let version = if source_version.is_empty() {
        "0.1.0".to_string()
    } else {
        source_version.clone()
    };
    let root_reference = if source_name.is_empty() || source_version.is_empty() {
        format!("{name}@npm")
    } else {
        format!("{name}#version={version}@npm")
    };
    plan.packages.push(ImportedPackage {
        name: name.clone(),
        version: version.clone(),
    });
    let root_facts = retain_normalized_root(
        &mut plan,
        ProviderFamily::Npm,
        package_json,
        &root_reference,
        "package.json",
    );
    for (field, dev, kind) in [
        ("dependencies", false, "runtime"),
        ("devDependencies", true, "dev"),
        ("optionalDependencies", false, "optional"),
        ("peerDependencies", false, "peer"),
    ] {
        let Some(JSONValue::Object(deps)) = obj.and_then(|m| m.get(field)) else {
            continue;
        };
        for (name, val) in deps {
            let requested = val.as_str().ok().map(str::to_string);
            let exact = requested.as_deref().and_then(exact_npm_version);
            let provider_ref = exact
                .as_deref()
                .map(|version| format!("{name}#version={version}@npm"))
                .unwrap_or_else(|| format!("{name}@npm"));
            plan.deps.push(ImportedDep {
                name: name.clone(),
                provider_ref: provider_ref.clone(),
                locked_version: exact.clone().unwrap_or_default(),
                dev,
            });
            if kind != "runtime" {
                plan.record_provider_finding(
                    "package.json".to_string(),
                    format!(
                        "npm `{kind}` dependency `{name}` has no canonical Jet runtime dependency output; migration remains unresolved"
                    ),
                );
            }
            let mut facts = ProviderFacts::for_reference("npm", &provider_ref);
            facts.set_native_document("package.json", package_json);
            facts.add_fact(
                "package.name",
                ProviderFactValue::Text(name.clone()),
                &format!("package.json.{field}"),
            );
            facts.add_fact(
                "package.dependency_kind",
                ProviderFactValue::Text(kind.to_string()),
                &format!("package.json.{field}"),
            );
            copy_provider_projection(&mut facts, &root_facts, "provider.npm.");
            match (requested, exact) {
                (Some(requested), Some(version)) => {
                    facts.add_fact(
                        "package.request",
                        ProviderFactValue::Text(requested),
                        &format!("package.json.{field}"),
                    );
                    facts.set_resolved_source(&format!("npm:{name}@{version}"));
                    facts.add_fact(
                        "package.version",
                        ProviderFactValue::Text(version),
                        "package.json exact selector",
                    );
                }
                (Some(requested), None) => {
                    facts.add_fact(
                        "package.request",
                        ProviderFactValue::Text(requested),
                        &format!("package.json.{field}"),
                    );
                    facts.add_loss(
                        "provider.selector",
                        "npm dependency request is not an exact lock identity",
                        &format!("package.json.{field}"),
                    );
                }
                (None, None) => facts.add_loss(
                    "package.request",
                    "npm dependency request must be a string",
                    &format!("package.json.{field}"),
                ),
                (None, Some(_)) => unreachable!("an exact npm version needs a string request"),
            }
            plan.retain_provider_facts_with_source(facts, "package.json");
        }
    }
    if let Some(JSONValue::Array(bundled)) = obj.and_then(|m| m.get("bundledDependencies")) {
        if !bundled.is_empty() {
            for (index, value) in bundled.iter().enumerate() {
                if !matches!(value, JSONValue::String(_)) {
                    plan.record_provider_finding(
                        "package.json".to_string(),
                        format!(
                            "npm `bundledDependencies[{index}]` is not a package name; migration is unresolved"
                        ),
                    );
                }
            }
            plan.record_provider_finding(
                "package.json".to_string(),
                "npm bundled dependencies need an explicit vendored migration mapping".to_string(),
            );
        }
    }
    if let Some(JSONValue::Object(scripts)) = obj.and_then(|m| m.get("scripts")) {
        for name in scripts.keys() {
            plan.todos.push(ImportTodo {
                source_path: "package.json".to_string(),
                message: format!("npm script `{name}` becomes a declared legacy build action"),
                suggested_surface: Some("Tier-2 legacy build action".to_string()),
            });
        }
    }
    plan
}

fn retain_normalized_root(
    plan: &mut ImportPlan,
    family: ProviderFamily,
    document: &str,
    reference: &str,
    source_path: &str,
) -> ProviderFacts {
    let report = normalize_provider_document(family, document);
    let facts = report.shared_facts_for(reference);
    plan.retain_provider_facts_with_source(facts.clone(), source_path);
    facts
}

fn copy_provider_projection(target: &mut ProviderFacts, source: &ProviderFacts, prefix: &str) {
    for (key, value) in &source.facts {
        if !key.starts_with(prefix) {
            continue;
        }
        let provenance = source
            .provenance
            .get(key)
            .map(String::as_str)
            .unwrap_or("provider.import");
        target.add_fact(key, value.clone(), provenance);
    }
}

pub fn import_python_metadata(name: &str, dynamic_fields: &[&str]) -> ImportPlan {
    let mut plan = ImportPlan::empty("pypi");
    plan.packages.push(ImportedPackage {
        name: name.to_string(),
        version: "0.1.0".to_string(),
    });
    for field in dynamic_fields {
        plan.todos.push(ImportTodo {
            source_path: "pyproject.toml".to_string(),
            message: format!("dynamic Python metadata field `{field}` must be resolved explicitly"),
            suggested_surface: None,
        });
    }
    let reference = format!("{name}@pypi");
    let mut facts = ProviderFacts::for_reference("pypi", &reference);
    facts.set_native_document("python-metadata", name);
    facts.add_loss(
        "provider.selector",
        "Python import has no exact distribution version",
        "pyproject.toml",
    );
    plan.retain_provider_facts(facts);
    plan
}

pub fn import_swiftpm(name: &str, revision: &str) -> ImportPlan {
    let mut plan = ImportPlan::empty("swiftpm");
    plan.packages.push(ImportedPackage {
        name: name.to_string(),
        version: revision.to_string(),
    });
    plan.deps.push(ImportedDep {
        name: name.to_string(),
        provider_ref: format!("{name}#revision={revision}@swiftpm"),
        locked_version: revision.to_string(),
        dev: false,
    });
    let provider_ref = format!("{name}#revision={revision}@swiftpm");
    let mut facts = ProviderFacts::for_reference("swiftpm", &provider_ref);
    facts.set_resolved_source(&format!("swiftpm:{name}@{revision}"));
    facts.set_native_document("Package.resolved", &provider_ref);
    facts.add_fact(
        "package.name",
        ProviderFactValue::Text(name.to_string()),
        "Package.resolved.identity",
    );
    facts.add_fact(
        "package.revision",
        ProviderFactValue::Text(revision.to_string()),
        "Package.resolved.revision",
    );
    plan.retain_provider_facts(facts);
    plan
}

pub fn merge_generated_file(
    existing: Option<&str>,
    generated: &GeneratedFile,
) -> Result<String, ImportConflict> {
    let Some(existing) = existing else {
        return Ok(generated.contents.clone());
    };
    if existing == generated.contents {
        return Ok(generated.contents.clone());
    }
    Err(ImportConflict::UserEditedGeneratedLine {
        path: generated.path.clone(),
    })
}

fn toml_string(raw: &str, key: &str) -> Option<String> {
    raw.lines().find_map(|line| {
        let line = line.trim();
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

fn cargo_dependency_sections(raw: &str) -> Vec<(String, bool, &'static str, Option<String>)> {
    let mut sections = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if !line.starts_with('[') || line.starts_with("[[") || !line.ends_with(']') {
            continue;
        }
        let section = line[1..line.len() - 1].trim();
        let Some((kind, suffix)) = [
            ("runtime", ".dependencies"),
            ("dev", ".dev-dependencies"),
            ("build", ".build-dependencies"),
        ]
        .into_iter()
        .find_map(|(kind, suffix)| {
            if section == suffix.trim_start_matches('.') {
                Some((kind, suffix))
            } else {
                section
                    .strip_prefix("target.")
                    .and_then(|target| target.strip_suffix(suffix))
                    .map(|_| (kind, suffix))
            }
        })
        else {
            continue;
        };
        let platform = section
            .strip_prefix("target.")
            .and_then(|target| target.strip_suffix(suffix))
            .map(|target| target.trim_matches(['\'', '"']).to_string());
        let header = format!("[{section}]");
        if !sections.iter().any(|(known, _, _, _)| known == &header) {
            sections.push((
                header,
                kind == "dev" || kind == "build",
                kind,
                platform,
            ));
        }
    }
    sections
}

fn dependency_keys(raw: &str, section: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_section = false;
    let wanted = section.trim_start_matches('[').trim_end_matches(']');
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            let current = line.trim_start_matches('[').trim_end_matches(']');
            in_section = current == wanted;
            continue;
        }
        if in_section {
            if let Some((key, _)) = line.split_once('=') {
                out.push(key.trim().to_string());
            }
        }
    }
    out
}

fn exact_npm_version(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-+".contains(character))
    {
        return None;
    }
    let core = value
        .split(|character: char| "-+".contains(character))
        .next()
        .unwrap_or_default();
    let parts = core.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    Some(value.to_string())
}

fn lock_versions(raw: &str, package: &str) -> Vec<String> {
    let mut versions = Vec::new();
    let mut in_package = false;
    let mut name = String::new();
    let mut version = String::new();
    let finish = |versions: &mut Vec<String>, name: &mut String, version: &mut String| {
        if name == package && !version.is_empty() {
            versions.push(version.clone());
        }
        name.clear();
        version.clear();
    };
    for line in raw.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            if in_package {
                finish(&mut versions, &mut name, &mut version);
            }
            in_package = true;
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "name" => name = value.trim().trim_matches('"').to_string(),
                "version" => version = value.trim().trim_matches('"').to_string(),
                _ => {}
            }
        }
    }
    if in_package {
        finish(&mut versions, &mut name, &mut version);
    }
    versions
}

fn lock_field(raw: &str, package: &str, field: &str) -> Option<String> {
    let mut in_package = false;
    let mut name = String::new();
    let mut value = None;
    for line in raw.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            if in_package && name == package && value.is_some() {
                return value;
            }
            in_package = true;
            name.clear();
            value = None;
            continue;
        }
        if !in_package {
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "name" => name = raw_value.trim().trim_matches('"').to_string(),
            key if key == field => value = Some(raw_value.trim().trim_matches('"').to_string()),
            _ => {}
        }
    }
    (in_package && name == package).then_some(value).flatten()
}

#[cfg(test)]
mod tests {
    use super::import_nix_facts;

    #[test]
    fn nix_import_production_path_retains_exact_provider_facts() {
        let plan = import_nix_facts(
            "flake.lock",
            r#"{"name":"app","version":"1.0.0","source":"nixpkgs","packages":[{"name":"ripgrep","version":"14.1.1","drvPath":"/nix/store/hash-ripgrep-14.1.1.drv"}]}"#,
        );
        let facts = &plan.provider_facts["ripgrep#version=14.1.1@nixpkgs"];
        facts.validate().expect("exact Nix import is lossless");
        assert!(plan
            .emit_pkg_jet()
            .contains("ripgrep: ripgrep#version=14.1.1@nixpkgs"));
        assert!(facts.native_document.contains("drvPath"));
        assert!(facts
            .facts
            .contains_key("provider.nix.import.drvPath"));
    }

    #[test]
    fn nix_import_production_path_reports_mutable_provider_facts() {
        let plan = import_nix_facts(
            "flake.nix",
            r#"{"name":"app","packages":["ripgrep"]}"#,
        );
        assert!(!plan.emit_pkg_jet().contains("ripgrep: ripgrep@nixpkgs"));
        assert!(plan.provider_facts["ripgrep@nixpkgs"]
            .losses
            .iter()
            .any(|loss| loss.reason.contains("no exact version")));
        assert!(plan
            .todos
            .iter()
            .any(|todo| todo.message.contains("migration remains unresolved")));
    }
}
